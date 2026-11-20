//! A Leitura de uma nota no cofre (sem Dioxus): resolver links e imagens contra o disco,
//! marcar tarefa sem pisar em edição de fora e abrir endereço no navegador do sistema.
//! O Markdown em si está em `markdown.rs`; aqui só entra o que precisa do cofre.

use std::cell::Cell;
use std::io;

use nota_sync::caminhos::dobrar;
use nota_sync::cofre as disco;

use super::cofre::{Cofre, Tipo};
use super::markdown::{self, Mundo};
use super::nomes::{pasta_de, ultimo_componente};

/// Quantas pastas e notas olhar no máximo ao montar o índice de nomes.
const MAX_VISTOS: usize = 20_000;
const MAX_NIVEIS: usize = 24;
/// Uma imagem maior que isto não é embutida (a WebView engasga e o HTML fica enorme).
pub const LIMITE_IMAGEM: u64 = 3 * 1024 * 1024;
/// Total de imagens embutidas por abertura de nota.
pub const LIMITE_IMAGENS_TOTAL: u64 = 12 * 1024 * 1024;

/// Chave para comparar nomes: sem distinguir caixa e em NFC (como o `Cofre`).
fn chave(s: &str) -> String {
    dobrar(&nota_cripto::normalizar(s).unwrap_or_else(|_| s.to_string()))
}

/// Junta `rel` a `base` resolvendo `.` e `..`. `None` se sai do cofre ou há componente vazio.
fn juntar_normalizado(base: &str, rel: &str) -> Option<String> {
    let mut partes: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                partes.pop()?;
            }
            outro => partes.push(outro),
        }
    }
    if partes.is_empty() {
        None
    } else {
        Some(partes.join("/"))
    }
}

/// Todas as notas do cofre (caminhos relativos, com `.md`), para achar `[[nome]]`.
fn todas_as_notas(cofre: &Cofre) -> Vec<String> {
    let mut notas = Vec::new();
    let mut vistos = 0usize;
    let mut pilha = vec![(String::new(), 0usize)];
    while let Some((pasta, nivel)) = pilha.pop() {
        let Ok(itens) = cofre.listar(&pasta) else {
            continue;
        };
        for item in itens {
            vistos += 1;
            if vistos > MAX_VISTOS {
                return notas;
            }
            match item.tipo {
                Tipo::Pasta if nivel < MAX_NIVEIS => pilha.push((item.caminho, nivel + 1)),
                Tipo::Pasta => {}
                Tipo::Nota => notas.push(item.caminho),
            }
        }
    }
    notas
}

/// O `Mundo` do Markdown apoiado no cofre de verdade. Monta o índice uma vez (ao abrir a nota).
pub struct MundoDoCofre {
    cofre: Cofre,
    /// (caminho, chave do caminho)
    notas: Vec<(String, String)>,
    imagens_embutidas: Cell<u64>,
}

impl MundoDoCofre {
    pub fn novo(cofre: &Cofre) -> Self {
        let notas = todas_as_notas(cofre)
            .into_iter()
            .map(|c| {
                let k = chave(&c);
                (c, k)
            })
            .collect();
        Self {
            cofre: cofre.clone(),
            notas,
            imagens_embutidas: Cell::new(0),
        }
    }

    #[cfg(test)]
    fn com_notas(cofre: &Cofre, notas: &[&str]) -> Self {
        Self {
            cofre: cofre.clone(),
            notas: notas.iter().map(|c| (c.to_string(), chave(c))).collect(),
            imagens_embutidas: Cell::new(0),
        }
    }

    fn achar_caminho(&self, rel: &str) -> Option<&str> {
        let k = chave(rel);
        self.notas
            .iter()
            .find(|(_, kc)| *kc == k)
            .map(|(c, _)| c.as_str())
    }
}

impl Mundo for MundoDoCofre {
    fn nota(&self, alvo: &str, de: &str) -> Option<String> {
        let alvo = alvo.trim().trim_matches('/');
        if alvo.is_empty() {
            return None;
        }
        let arquivo = format!("{alvo}.md");
        if alvo.contains('/') {
            // Caminho: a partir da pasta da nota, depois a partir da raiz do cofre.
            for base in [pasta_de(de), ""] {
                if let Some(p) = juntar_normalizado(base, &arquivo)
                    && let Some(c) = self.achar_caminho(&p)
                {
                    return Some(c.to_string());
                }
            }
            // Como no Obsidian: o fim do caminho basta (`Pasta/Nota` acha `Tudo/Pasta/Nota.md`).
            let fim = format!("/{}", chave(&arquivo));
            return self
                .notas
                .iter()
                .filter(|(_, k)| k.ends_with(&fim))
                .min_by_key(|(c, _)| (c.matches('/').count(), c.clone()))
                .map(|(c, _)| c.clone());
        }
        // Só o nome: vale em qualquer pasta. Vence a da mesma pasta, depois a mais rasa.
        let k = chave(&arquivo);
        let pasta = pasta_de(de);
        self.notas
            .iter()
            .filter(|(c, _)| chave(ultimo_componente(c)) == k)
            .min_by_key(|(c, _)| (pasta_de(c) != pasta, c.matches('/').count(), c.clone()))
            .map(|(c, _)| c.clone())
    }

    fn imagem(&self, origem: &str, de: &str) -> Option<String> {
        let origem = origem.trim();
        if origem.is_empty() || origem.starts_with('/') {
            return None;
        }
        let tipo = tipo_pela_extensao(ultimo_componente(origem))?;
        for base in [pasta_de(de), ""] {
            let Some(rel) = juntar_normalizado(base, origem) else {
                continue;
            };
            // `caminho_dentro` recusa symlink e qualquer coisa fora do cofre.
            let Ok(caminho) = disco::caminho_dentro(self.cofre.raiz(), &rel) else {
                continue;
            };
            let Ok(meta) = std::fs::symlink_metadata(&caminho) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let usado = self.imagens_embutidas.get();
            if meta.len() > LIMITE_IMAGEM || usado + meta.len() > LIMITE_IMAGENS_TOTAL {
                return None;
            }
            let Ok(Some(bytes)) = disco::ler(self.cofre.raiz(), &rel) else {
                continue;
            };
            if !confere_assinatura(&bytes, tipo) {
                return None;
            }
            self.imagens_embutidas.set(usado + bytes.len() as u64);
            return Some(format!("data:{tipo};base64,{}", base64(&bytes)));
        }
        None
    }
}

/// O tipo de imagem pela extensão (só os que a WebView desenha sem risco; SVG fica de fora).
fn tipo_pela_extensao(nome: &str) -> Option<&'static str> {
    let minusculo = nome.to_ascii_lowercase();
    let ext = minusculo.rsplit_once('.')?.1;
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Os primeiros bytes têm de ser do tipo que a extensão diz (um `.png` que é outra coisa não entra).
fn confere_assinatura(b: &[u8], tipo: &str) -> bool {
    match tipo {
        "image/png" => b.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => b.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a"),
        "image/webp" => b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP",
        _ => false,
    }
}

const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 padrão (com `=`), para o `data:` das imagens.
pub fn base64(entrada: &[u8]) -> String {
    let mut s = String::with_capacity(entrada.len().div_ceil(3) * 4);
    for bloco in entrada.chunks(3) {
        let n = match *bloco {
            [a, b, c] => (a as u32) << 16 | (b as u32) << 8 | c as u32,
            [a, b] => (a as u32) << 16 | (b as u32) << 8,
            [a] => (a as u32) << 16,
            _ => 0,
        };
        s.push(ALFABETO[(n >> 18) as usize & 63] as char);
        s.push(ALFABETO[(n >> 12) as usize & 63] as char);
        s.push(if bloco.len() > 1 {
            ALFABETO[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if bloco.len() > 2 {
            ALFABETO[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

/// Por que a caixa de tarefa não foi gravada.
#[derive(Debug)]
pub enum ErroTarefa {
    /// O arquivo é diferente do que a tela mostra (mudou por fora): nada foi gravado.
    Mudou,
    /// No lugar da caixa não há mais uma caixa.
    Sumiu,
    Disco(io::Error),
}

/// Marca ou desmarca a tarefa em `byte` e grava a nota (de forma atômica, como o editor).
/// `base` é o texto que a tela está mostrando: se o arquivo no disco já é outro, não grava.
/// Devolve o texto novo.
pub fn alternar_tarefa(
    cofre: &Cofre,
    caminho: &str,
    base: &str,
    byte: usize,
) -> Result<String, ErroTarefa> {
    let atual = cofre.ler_nota(caminho).map_err(ErroTarefa::Disco)?;
    if atual != base {
        return Err(ErroTarefa::Mudou);
    }
    let novo = markdown::alternar_tarefa(base, byte).ok_or(ErroTarefa::Sumiu)?;
    // Conferir de novo e gravar com o portão preso: o ciclo de sincronização não troca o
    // arquivo entre as duas coisas.
    if !cofre
        .salvar_se_igual(caminho, base, &novo)
        .map_err(ErroTarefa::Disco)?
    {
        return Err(ErroTarefa::Mudou);
    }
    Ok(novo)
}

/// Abre um endereço `http(s)` no navegador do sistema, sem JNI: no Android pede ao `am start`
/// (o utilitário do sistema) uma `VIEW` para o endereço. O endereço vai como argumento, nunca
/// por shell. Fora do Android devolve `Unsupported` (a tela mostra o endereço).
pub fn abrir_no_navegador(url: &str) -> io::Result<()> {
    let Some(url) = markdown::url_web(url) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "só abro endereços http e https",
        ));
    };
    abrir_url_do_sistema(&url)
}

#[cfg(target_os = "android")]
fn abrir_url_do_sistema(url: &str) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let mut filho = Command::new("am")
        .args(["start", "-a", "android.intent.action.VIEW", "-d", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Recolhe o processo para não deixar zumbi; não bloqueia a tela.
    std::thread::spawn(move || {
        let _ = filho.wait();
    });
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn abrir_url_do_sistema(_url: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "o navegador só abre no Android",
    ))
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::fs;

    fn cofre() -> (tempfile::TempDir, Cofre) {
        let t = tempfile::tempdir().unwrap();
        let c = Cofre::abrir(t.path().join("Cofre")).unwrap();
        (t, c)
    }

    /// Um PNG mínimo (só a assinatura importa para a conferência).
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn base64_confere_com_os_exemplos_da_rfc() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn normalizar_caminhos() {
        assert_eq!(
            juntar_normalizado("a/b", "c.md").as_deref(),
            Some("a/b/c.md")
        );
        assert_eq!(
            juntar_normalizado("a/b", "../c.md").as_deref(),
            Some("a/c.md")
        );
        assert_eq!(juntar_normalizado("a", "./x/./y").as_deref(), Some("a/x/y"));
        assert_eq!(juntar_normalizado("", "x.md").as_deref(), Some("x.md"));
        assert_eq!(juntar_normalizado("a", "../../x"), None);
        assert_eq!(juntar_normalizado("", ".."), None);
    }

    #[test]
    fn acha_nota_por_nome_em_qualquer_pasta_e_sem_distinguir_caixa() {
        let (_t, c) = cofre();
        let m = MundoDoCofre::com_notas(
            &c,
            &[
                "Raiz.md",
                "Trabalho/Projeto.md",
                "Casa/Projeto.md",
                "Casa/Obras/Reforma.md",
            ],
        );
        assert_eq!(m.nota("Raiz", "x.md").as_deref(), Some("Raiz.md"));
        assert_eq!(m.nota("raiz", "x.md").as_deref(), Some("Raiz.md"));
        assert_eq!(
            m.nota("reforma", "x.md").as_deref(),
            Some("Casa/Obras/Reforma.md")
        );
        // Nome repetido: a da mesma pasta vence; sem isso, a mais rasa e a primeira em ordem.
        assert_eq!(
            m.nota("Projeto", "Trabalho/a.md").as_deref(),
            Some("Trabalho/Projeto.md")
        );
        assert_eq!(
            m.nota("Projeto", "Casa/b.md").as_deref(),
            Some("Casa/Projeto.md")
        );
        assert_eq!(
            m.nota("Projeto", "outra.md").as_deref(),
            Some("Casa/Projeto.md")
        );
        assert_eq!(m.nota("Fantasma", "x.md"), None);
        assert_eq!(m.nota("", "x.md"), None);
    }

    #[test]
    fn acha_nota_por_caminho_relativo_raiz_ou_final() {
        let (_t, c) = cofre();
        let m = MundoDoCofre::com_notas(
            &c,
            &[
                "Casa/Obras/Reforma.md",
                "Casa/Plano.md",
                "Plano.md",
                "Tudo/Casa/Plano.md",
            ],
        );
        // Relativo à pasta da nota.
        assert_eq!(
            m.nota("Obras/Reforma", "Casa/x.md").as_deref(),
            Some("Casa/Obras/Reforma.md")
        );
        assert_eq!(
            m.nota("../Plano", "Casa/Obras/x.md").as_deref(),
            Some("Casa/Plano.md")
        );
        // A partir da raiz.
        assert_eq!(
            m.nota("Casa/Plano", "Outro/x.md").as_deref(),
            Some("Casa/Plano.md")
        );
        // Só o fim do caminho.
        assert_eq!(
            m.nota("Obras/Reforma", "x.md").as_deref(),
            Some("Casa/Obras/Reforma.md")
        );
        assert_eq!(m.nota("../../..", "x.md"), None);
    }

    #[test]
    fn indice_de_verdade_acha_e_esconde_lixeira() {
        let (_t, c) = cofre();
        c.salvar_nota("Projetos/Alfa.md", "x").unwrap();
        c.salvar_nota("Raiz.md", "x").unwrap();
        c.salvar_nota(".trash/Velha 2026.md", "x").unwrap();
        let m = MundoDoCofre::novo(&c);
        assert_eq!(
            m.nota("alfa", "Raiz.md").as_deref(),
            Some("Projetos/Alfa.md")
        );
        assert_eq!(m.nota("Velha 2026", "Raiz.md"), None);
    }

    #[test]
    fn imagem_dentro_do_cofre_vira_data_uri() {
        let (_t, c) = cofre();
        fs::create_dir_all(c.raiz().join("Fotos")).unwrap();
        fs::write(c.raiz().join("Fotos/gato.PNG"), PNG).unwrap();
        fs::write(c.raiz().join("raiz.png"), PNG).unwrap();
        let m = MundoDoCofre::novo(&c);
        let esperado = format!("data:image/png;base64,{}", base64(PNG));
        // Relativa à pasta da nota e a partir da raiz.
        assert_eq!(
            m.imagem("gato.PNG", "Fotos/n.md").as_deref(),
            Some(esperado.as_str())
        );
        assert_eq!(
            m.imagem("Fotos/gato.PNG", "n.md").as_deref(),
            Some(esperado.as_str())
        );
        assert_eq!(
            m.imagem("raiz.png", "Fotos/n.md").as_deref(),
            Some(esperado.as_str())
        );
        assert_eq!(
            m.imagem("../raiz.png", "Fotos/n.md").as_deref(),
            Some(esperado.as_str())
        );
    }

    #[test]
    fn imagem_fora_do_cofre_ou_estranha_nao_entra() {
        let (t, c) = cofre();
        fs::write(t.path().join("fora.png"), PNG).unwrap();
        fs::write(c.raiz().join("falso.png"), b"<script>alert(1)</script>").unwrap();
        fs::write(c.raiz().join("desenho.svg"), b"<svg onload=alert(1)>").unwrap();
        fs::write(c.raiz().join("texto.md"), PNG).unwrap();
        let m = MundoDoCofre::novo(&c);
        // `..` para fora do cofre, caminho absoluto, extensão que não é imagem, conteúdo que não bate.
        assert_eq!(m.imagem("../fora.png", "n.md"), None);
        assert_eq!(m.imagem("../../fora.png", "A/n.md"), None);
        assert_eq!(m.imagem("/etc/passwd.png", "n.md"), None);
        assert_eq!(m.imagem("C:/x.png", "n.md"), None);
        assert_eq!(m.imagem("desenho.svg", "n.md"), None);
        assert_eq!(m.imagem("texto.md", "n.md"), None);
        assert_eq!(m.imagem("falso.png", "n.md"), None);
        assert_eq!(m.imagem("nao-existe.png", "n.md"), None);
        assert_eq!(m.imagem("", "n.md"), None);
    }

    #[test]
    fn imagem_grande_ou_demais_nao_entra() {
        let (_t, c) = cofre();
        let f = fs::File::create(c.raiz().join("grande.png")).unwrap();
        f.set_len(LIMITE_IMAGEM + 1).unwrap();
        let m = MundoDoCofre::novo(&c);
        assert_eq!(m.imagem("grande.png", "n.md"), None);
        // O total por abertura também é limitado.
        fs::write(c.raiz().join("p.png"), PNG).unwrap();
        m.imagens_embutidas.set(LIMITE_IMAGENS_TOTAL);
        assert_eq!(m.imagem("p.png", "n.md"), None);
    }

    #[cfg(unix)]
    #[test]
    fn imagem_por_symlink_nao_entra() {
        let (t, c) = cofre();
        fs::write(t.path().join("segredo.png"), PNG).unwrap();
        std::os::unix::fs::symlink(t.path().join("segredo.png"), c.raiz().join("atalho.png"))
            .unwrap();
        let m = MundoDoCofre::novo(&c);
        assert_eq!(m.imagem("atalho.png", "n.md"), None);
    }

    #[test]
    fn tarefa_grava_so_o_colchete_e_de_forma_limpa() {
        let (_t, c) = cofre();
        let texto = "# Compras\n\n- [ ] pão\n- [x] leite\n- item [ ] solto\n";
        c.salvar_nota("c.md", texto).unwrap();
        let m = MundoDoCofre::novo(&c);
        let p = markdown::renderizar(texto, "c.md", &m);
        let novo = alternar_tarefa(&c, "c.md", texto, p.tarefas[0].byte).unwrap();
        assert_eq!(
            novo,
            "# Compras\n\n- [x] pão\n- [x] leite\n- item [ ] solto\n"
        );
        assert_eq!(c.ler_nota("c.md").unwrap(), novo);
        // Desmarcar a outra, a partir do texto novo.
        let p2 = markdown::renderizar(&novo, "c.md", &m);
        let novo2 = alternar_tarefa(&c, "c.md", &novo, p2.tarefas[1].byte).unwrap();
        assert_eq!(
            novo2,
            "# Compras\n\n- [x] pão\n- [ ] leite\n- item [ ] solto\n"
        );
        // Sem temporário sobrando.
        assert_eq!(fs::read_dir(c.raiz()).unwrap().count(), 1);
    }

    #[test]
    fn tarefa_nao_grava_se_o_arquivo_mudou_por_fora() {
        let (_t, c) = cofre();
        let texto = "- [ ] a\n- [ ] b\n";
        c.salvar_nota("c.md", texto).unwrap();
        let p = markdown::renderizar(texto, "c.md", &MundoDoCofre::novo(&c));
        // Outro aparelho (sync) mexeu na nota depois que a tela abriu.
        let por_fora = "- [ ] a\n- [ ] b\n- [ ] c nova\n";
        c.salvar_nota("c.md", por_fora).unwrap();
        let r = alternar_tarefa(&c, "c.md", texto, p.tarefas[0].byte);
        assert!(matches!(r, Err(ErroTarefa::Mudou)), "{r:?}");
        assert_eq!(c.ler_nota("c.md").unwrap(), por_fora);
        // Mudança de um byte só (outro aparelho marcou a mesma) também conta.
        c.salvar_nota("c.md", "- [x] a\n- [ ] b\n").unwrap();
        let r = alternar_tarefa(&c, "c.md", texto, p.tarefas[0].byte);
        assert!(matches!(r, Err(ErroTarefa::Mudou)));
        assert_eq!(c.ler_nota("c.md").unwrap(), "- [x] a\n- [ ] b\n");
    }

    #[test]
    fn tarefa_sem_caixa_no_lugar_ou_nota_sumida_nao_grava() {
        let (_t, c) = cofre();
        c.salvar_nota("c.md", "texto sem tarefa\n").unwrap();
        let r = alternar_tarefa(&c, "c.md", "texto sem tarefa\n", 0);
        assert!(matches!(r, Err(ErroTarefa::Sumiu)), "{r:?}");
        let r = alternar_tarefa(&c, "sumiu.md", "x", 0);
        assert!(matches!(r, Err(ErroTarefa::Disco(_))), "{r:?}");
        assert_eq!(c.ler_nota("c.md").unwrap(), "texto sem tarefa\n");
    }

    #[test]
    fn navegador_so_recebe_http_e_https() {
        for ruim in [
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "intent://x#Intent;end",
            "-d x",
            "",
        ] {
            let e = abrir_no_navegador(ruim).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{ruim:?}");
        }
        // No Windows não há navegador a abrir: o erro é "não suportado", nunca pânico.
        #[cfg(not(target_os = "android"))]
        assert_eq!(
            abrir_no_navegador("https://exemplo.com")
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
    }
}
