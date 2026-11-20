//! O cofre no disco: listar, ler, salvar, criar, renomear e mandar para a lixeira.
//!
//! Reusa o que o sincronizador já garante (`nota_sync::cofre`): caminho que nunca sai do cofre,
//! escrita atômica (temporário + rename) e lixeira em `.trash/` (nada é apagado de vez).
//! Os erros são `io::Error`; a tela mostra `explicar(&erro)`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nota_sync::caminhos::{dobrar, eh_ignorado, nome_conflito};
use nota_sync::cofre::{self as disco, hash_de};
use nota_sync::motor::Hash;

use super::nomes::{
    PASTA_NOVA, TITULO_NOVO, arquivo_da_nota, eh_nota, juntar, limpar_nome, nome_unico, pasta_de,
    titulo_da_nota, ultimo_componente,
};
use super::ordem::comparar;
use super::portao::PortaoComum;

/// Nota maior que isto não abre no editor (a WebView engasga e a gravação fica pesada).
pub const LIMITE_NOTA: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Pasta,
    Nota,
}

/// Uma linha da lista.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// O que aparece: nome da pasta, ou título da nota (sem `.md`).
    pub nome: String,
    /// Caminho relativo ao cofre, com `/` (com `.md` nas notas).
    pub caminho: String,
    pub tipo: Tipo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cofre {
    raiz: PathBuf,
    /// Toda gravação do app toma este portão; o ciclo de sincronização também (ver `portao`).
    portao: PortaoComum,
}

/// O que aconteceu ao gravar o texto do editor (`salvar_nota_conferindo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gravou {
    /// Gravou na própria nota; este é o hash do que ficou no disco.
    Normal(Hash),
    /// A nota mudou no disco depois de aberta (sincronização, outro app): o arquivo novo
    /// ficou como está e o texto do dono foi para esta cópia de conflito.
    Desviou { copia: String, hash: Hash },
}

fn erro(tipo: io::ErrorKind, msg: &str) -> io::Error {
    io::Error::new(tipo, msg.to_string())
}

/// Mensagem em português para mostrar ao dono.
pub fn explicar(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "Não encontrei o arquivo (foi movido ou apagado?).".into(),
        io::ErrorKind::PermissionDenied => "O Android não deixou o app acessar esta pasta.".into(),
        io::ErrorKind::StorageFull => "Sem espaço no celular.".into(),
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => e.to_string(),
        _ => format!("Erro no arquivo: {e}"),
    }
}

/// Chave para comparar nomes: o Android (e o Windows) não distinguem maiúscula, e o nome pode
/// estar decomposto (NFD) no disco.
fn chave(nome: &str) -> String {
    dobrar(&nota_cripto::normalizar(nome).unwrap_or_else(|_| nome.to_string()))
}

impl Cofre {
    /// Abre (e cria, se faltar) a pasta do cofre.
    pub fn abrir(raiz: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&raiz)?;
        Ok(Self {
            raiz,
            portao: PortaoComum::default(),
        })
    }

    pub fn portao(&self) -> &PortaoComum {
        &self.portao
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    fn dir(&self, pasta: &str) -> io::Result<PathBuf> {
        if pasta.is_empty() {
            Ok(self.raiz.clone())
        } else {
            disco::caminho_dentro(&self.raiz, pasta)
        }
    }

    /// Pastas e notas de `pasta` (`""` = raiz): pastas primeiro, cada grupo em ordem natural.
    /// Esconde tudo que começa com ponto (`.trash`, `.obsidian`...), temporários, links
    /// simbólicos e arquivos que não são `.md`.
    pub fn listar(&self, pasta: &str) -> io::Result<Vec<Item>> {
        let mut pastas = Vec::new();
        let mut notas = Vec::new();
        for entrada in fs::read_dir(self.dir(pasta)?)? {
            let entrada = entrada?;
            let Ok(nome) = entrada.file_name().into_string() else {
                continue;
            };
            let caminho = juntar(pasta, &nome);
            if nome.starts_with('.') || eh_ignorado(&caminho) {
                continue;
            }
            let Ok(tipo) = entrada.file_type() else {
                continue;
            };
            if tipo.is_symlink() {
                continue;
            }
            if tipo.is_dir() {
                pastas.push(Item {
                    nome,
                    caminho,
                    tipo: Tipo::Pasta,
                });
            } else if tipo.is_file() && eh_nota(&nome) {
                notas.push(Item {
                    nome: titulo_da_nota(&nome).to_string(),
                    caminho,
                    tipo: Tipo::Nota,
                });
            }
        }
        pastas.sort_by(|a, b| comparar(&a.nome, &b.nome));
        notas.sort_by(|a, b| comparar(&a.nome, &b.nome));
        pastas.extend(notas);
        Ok(pastas)
    }

    /// Lê a nota como texto. Recusa o que não é UTF-8 ou é grande demais: abrir e salvar de
    /// volta estragaria o arquivo.
    pub fn ler_nota(&self, rel: &str) -> io::Result<String> {
        let tamanho = fs::metadata(disco::caminho_dentro(&self.raiz, rel)?)?.len();
        if tamanho > LIMITE_NOTA {
            return Err(erro(
                io::ErrorKind::InvalidData,
                "Esta nota é grande demais para editar no celular (mais de 2 MB).",
            ));
        }
        let bytes =
            disco::ler(&self.raiz, rel)?.ok_or_else(|| erro(io::ErrorKind::NotFound, ""))?;
        String::from_utf8(bytes).map_err(|_| {
            erro(
                io::ErrorKind::InvalidData,
                "Esta nota não está em UTF-8; não abri para não estragar o arquivo.",
            )
        })
    }

    /// Grava a nota por inteiro, de forma atômica, com o portão preso. (O app grava pelo
    /// `salvar_nota_conferindo`, `salvar_se_igual` e `criar_nota`; esta é a forma simples que os
    /// testes usam para montar um cofre.)
    #[cfg(test)]
    pub fn salvar_nota(&self, rel: &str, texto: &str) -> io::Result<()> {
        let _preso = self.portao.0.prender();
        self.gravar(rel, texto)
    }

    /// A gravação em si; quem chama já está com o portão.
    fn gravar(&self, rel: &str, texto: &str) -> io::Result<()> {
        disco::escrever_atomico(&self.raiz, rel, texto.as_bytes())?;
        self.portao.0.contou_gravacao();
        Ok(())
    }

    /// O hash do arquivo no disco agora (`None` se não existe).
    pub fn hash_da_nota(&self, rel: &str) -> io::Result<Option<Hash>> {
        Ok(disco::ler(&self.raiz, rel)?.map(|b| hash_de(&b)))
    }

    /// Grava o texto que o dono digitou **só se a nota ainda é a que ele abriu** (`base` é o
    /// hash do que o app leu ou gravou por último). Se o arquivo mudou no disco (o sync trouxe
    /// outra versão, outro app mexeu), não pisa nele: o texto do dono vai para uma cópia
    /// `nome (conflito <aparelho> <data>).md` e a versão nova fica onde estava. Conferir e
    /// gravar acontecem com o portão preso, então o ciclo não troca o arquivo no meio.
    /// Nota que sumiu do disco (apagada em outro aparelho) é recriada com o texto do dono.
    pub fn salvar_nota_conferindo(
        &self,
        rel: &str,
        texto: &str,
        base: Option<Hash>,
    ) -> io::Result<Gravou> {
        let carimbo = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
        self.salvar_conferindo_com(rel, texto, base, &carimbo)
    }

    /// Igual a `salvar_nota_conferindo`, com o carimbo escolhido (para os testes).
    pub fn salvar_conferindo_com(
        &self,
        rel: &str,
        texto: &str,
        base: Option<Hash>,
        carimbo: &str,
    ) -> io::Result<Gravou> {
        let _preso = self.portao.0.prender();
        let novo = hash_de(texto.as_bytes());
        let atual = self.hash_da_nota(rel)?;
        let pode = match (atual, base) {
            (None, _) => true,
            (Some(a), Some(b)) => a == b || a == novo,
            (Some(a), None) => a == novo,
        };
        if pode {
            if atual != Some(novo) {
                self.gravar(rel, texto)?;
            }
            return Ok(Gravou::Normal(novo));
        }
        let nome = nome_conflito(rel, &self.portao.0.aparelho(), carimbo);
        let copia = disco::gravar_unico(&self.raiz, &nome, texto.as_bytes())?;
        self.portao.0.contou_gravacao();
        Ok(Gravou::Desviou { copia, hash: novo })
    }

    /// Troca o conteúdo da nota por `novo` **só se** ela ainda tem exatamente `esperado`
    /// (conferir e gravar com o portão preso). `Ok(false)` = mudou por fora, nada gravado.
    pub fn salvar_se_igual(&self, rel: &str, esperado: &str, novo: &str) -> io::Result<bool> {
        let _preso = self.portao.0.prender();
        if self.hash_da_nota(rel)? != Some(hash_de(esperado.as_bytes())) {
            return Ok(false);
        }
        self.gravar(rel, novo)?;
        Ok(true)
    }

    /// Nomes (de arquivo ou pasta) que já existem em `pasta`, na forma de comparação.
    fn chaves_em(&self, pasta: &str) -> io::Result<Vec<String>> {
        let mut v = Vec::new();
        for e in fs::read_dir(self.dir(pasta)?)? {
            if let Ok(nome) = e?.file_name().into_string() {
                v.push(chave(&nome));
            }
        }
        Ok(v)
    }

    /// Cria uma nota vazia em `pasta` chamada `Sem título`, `Sem título 2`... Devolve o caminho.
    pub fn criar_nota(&self, pasta: &str) -> io::Result<String> {
        let _preso = self.portao.0.prender();
        let usados = self.chaves_em(pasta)?;
        let titulo = nome_unico(
            |n| usados.contains(&chave(&arquivo_da_nota(n))),
            TITULO_NOVO,
        );
        let rel = juntar(pasta, &arquivo_da_nota(&titulo));
        self.gravar(&rel, "")?;
        Ok(rel)
    }

    /// Cria uma pasta em `pasta`. Se o nome já existe, vira `nome 2`. Devolve o caminho.
    pub fn criar_pasta(&self, pasta: &str, nome: &str) -> io::Result<String> {
        let _preso = self.portao.0.prender();
        let base = limpar_nome(nome).unwrap_or_else(|| PASTA_NOVA.to_string());
        let usados = self.chaves_em(pasta)?;
        let nome = nome_unico(|n| usados.contains(&chave(n)), &base);
        let rel = juntar(pasta, &nome);
        let destino = disco::caminho_dentro(&self.raiz, &rel)?;
        fs::create_dir(&destino)?;
        self.portao.0.contou_gravacao();
        Ok(rel)
    }

    /// Renomeia a nota para `titulo_novo` (sem `.md`), na mesma pasta. Se já existe uma nota com
    /// esse nome, a renomeada vira `nome 2`: nunca sobrescreve. Devolve o caminho novo.
    pub fn renomear_nota(&self, rel: &str, titulo_novo: &str) -> io::Result<String> {
        // Conferir o nome e fazer o `rename` sem o ciclo de sincronização no meio.
        let _preso = self.portao.0.prender();
        let titulo = limpar_nome(titulo_novo).ok_or_else(|| {
            erro(
                io::ErrorKind::InvalidInput,
                "Esse nome não serve. Use letras e números, sem barras.",
            )
        })?;
        let pasta = pasta_de(rel);
        let atual = ultimo_componente(rel);
        let novo_arquivo = arquivo_da_nota(&titulo);
        if novo_arquivo == atual {
            return Ok(rel.to_string());
        }
        // Só a caixa mudou (`ideia` -> `Ideia`): é a própria nota, não é colisão.
        let arquivo = if chave(&novo_arquivo) == chave(atual) {
            novo_arquivo
        } else {
            let usados = self.chaves_em(pasta)?;
            let t = nome_unico(|n| usados.contains(&chave(&arquivo_da_nota(n))), &titulo);
            arquivo_da_nota(&t)
        };
        let novo_rel = juntar(pasta, &arquivo);
        let origem = disco::caminho_dentro(&self.raiz, rel)?;
        let destino = disco::caminho_dentro(&self.raiz, &novo_rel)?;
        fs::rename(origem, destino)?;
        self.portao.0.contou_gravacao();
        Ok(novo_rel)
    }

    /// Manda a nota ou pasta para `.trash/` com o carimbo de agora. Nunca apaga de vez.
    pub fn para_a_lixeira(&self, rel: &str) -> io::Result<String> {
        let carimbo = chrono::Local::now().format("%Y-%m-%d %H%M%S").to_string();
        self.para_a_lixeira_com(rel, &carimbo)
    }

    /// Igual a `para_a_lixeira`, com o carimbo escolhido (para os testes).
    pub fn para_a_lixeira_com(&self, rel: &str, carimbo: &str) -> io::Result<String> {
        let _preso = self.portao.0.prender();
        let destino = disco::mover_para_lixeira(&self.raiz, rel, carimbo)?;
        self.portao.0.contou_gravacao();
        Ok(destino)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use tempfile::TempDir;

    fn cofre() -> (TempDir, Cofre) {
        let t = tempfile::tempdir().unwrap();
        let c = Cofre::abrir(t.path().join("Cofre")).unwrap();
        (t, c)
    }

    fn nomes(c: &Cofre, pasta: &str) -> Vec<String> {
        c.listar(pasta)
            .unwrap()
            .into_iter()
            .map(|i| i.nome)
            .collect()
    }

    #[test]
    fn lista_pastas_primeiro_e_esconde_o_resto() {
        let (_t, c) = cofre();
        c.salvar_nota("b.md", "x").unwrap();
        c.salvar_nota("a.md", "x").unwrap();
        c.salvar_nota("Nota 10.md", "x").unwrap();
        c.salvar_nota("Nota 2.md", "x").unwrap();
        c.criar_pasta("", "Zeta").unwrap();
        c.criar_pasta("", "alfa").unwrap();
        // Coisas que não aparecem.
        fs::write(c.raiz().join("foto.png"), b"x").unwrap();
        fs::write(c.raiz().join(".oculto.md"), b"x").unwrap();
        fs::write(c.raiz().join("lixo.md.tmp"), b"x").unwrap();
        fs::create_dir(c.raiz().join(".obsidian")).unwrap();
        fs::create_dir(c.raiz().join(".trash")).unwrap();
        assert_eq!(
            nomes(&c, ""),
            ["alfa", "Zeta", "a", "b", "Nota 2", "Nota 10"]
        );
    }

    #[test]
    fn lista_subpasta_e_recusa_caminho_fora() {
        let (_t, c) = cofre();
        let p = c.criar_pasta("", "Trabalho").unwrap();
        c.salvar_nota("Trabalho/ata.md", "x").unwrap();
        assert_eq!(nomes(&c, &p), ["ata"]);
        assert!(c.listar("../fora").is_err());
        assert!(c.listar("nao-existe").is_err());
    }

    #[test]
    fn nota_nova_nunca_sobrescreve() {
        let (_t, c) = cofre();
        let a = c.criar_nota("").unwrap();
        c.salvar_nota(&a, "meu texto").unwrap();
        let b = c.criar_nota("").unwrap();
        let d = c.criar_nota("").unwrap();
        assert_eq!(
            (a.as_str(), b.as_str(), d.as_str()),
            ("Sem título.md", "Sem título 2.md", "Sem título 3.md")
        );
        assert_eq!(c.ler_nota(&a).unwrap(), "meu texto");
    }

    #[test]
    fn nota_nova_em_pasta_e_caixa_diferente() {
        let (_t, c) = cofre();
        let p = c.criar_pasta("", "Diário").unwrap();
        c.salvar_nota("Diário/sem título.md", "x").unwrap();
        // Maiúscula/minúscula conta como o mesmo nome (Android e Windows).
        assert_eq!(c.criar_nota(&p).unwrap(), "Diário/Sem título 2.md");
    }

    #[test]
    fn pasta_nova_nome_invalido_vira_padrao_e_nao_colide() {
        let (_t, c) = cofre();
        assert_eq!(c.criar_pasta("", "  ").unwrap(), "Nova pasta");
        assert_eq!(c.criar_pasta("", "Nova pasta").unwrap(), "Nova pasta 2");
        assert_eq!(c.criar_pasta("", "a/b").unwrap(), "a-b");
    }

    #[test]
    fn salva_e_le_com_acentos_e_emoji() {
        let (_t, c) = cofre();
        let texto = "ação, pão e açúcar 😀\nsegunda linha\n";
        c.salvar_nota("x.md", texto).unwrap();
        assert_eq!(c.ler_nota("x.md").unwrap(), texto);
        // Não deixa temporário para trás.
        let restos: Vec<_> = fs::read_dir(c.raiz())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(restos.len(), 1);
    }

    #[test]
    fn recusa_nota_que_nao_e_utf8_e_nota_gigante() {
        let (_t, c) = cofre();
        fs::write(c.raiz().join("ruim.md"), [0xff, 0xfe, 0x41]).unwrap();
        assert_eq!(
            c.ler_nota("ruim.md").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let f = fs::File::create(c.raiz().join("grande.md")).unwrap();
        f.set_len(LIMITE_NOTA + 1).unwrap();
        assert_eq!(
            c.ler_nota("grande.md").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            c.ler_nota("faltou.md").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn renomear_sem_colidir() {
        let (_t, c) = cofre();
        c.salvar_nota("a.md", "texto A").unwrap();
        c.salvar_nota("b.md", "texto B").unwrap();
        // Para um nome livre.
        assert_eq!(c.renomear_nota("a.md", "Ideias").unwrap(), "Ideias.md");
        assert_eq!(c.ler_nota("Ideias.md").unwrap(), "texto A");
        // Para um nome que já existe: vira "B 2", e a nota b continua intacta.
        assert_eq!(c.renomear_nota("Ideias.md", "B").unwrap(), "B 2.md");
        assert_eq!(c.ler_nota("b.md").unwrap(), "texto B");
        assert_eq!(c.ler_nota("B 2.md").unwrap(), "texto A");
        // Mesmo nome: não faz nada. Só a caixa: renomeia.
        assert_eq!(c.renomear_nota("b.md", "b").unwrap(), "b.md");
        assert_eq!(c.renomear_nota("b.md", "Bê").unwrap(), "Bê.md");
        // Nome inválido.
        assert_eq!(
            c.renomear_nota("B 2.md", "  ").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn renomear_dentro_de_pasta_e_so_caixa() {
        let (_t, c) = cofre();
        c.criar_pasta("", "P").unwrap();
        c.salvar_nota("P/ideia.md", "x").unwrap();
        assert_eq!(
            c.renomear_nota("P/ideia.md", "Ideia.md").unwrap(),
            "P/Ideia.md"
        );
        assert_eq!(nomes(&c, "P"), ["Ideia"]);
    }

    #[test]
    fn lixeira_move_nota_e_pasta_sem_apagar() {
        let (_t, c) = cofre();
        c.salvar_nota("a.md", "conteúdo").unwrap();
        let na_lixeira = c.para_a_lixeira_com("a.md", "2026-11-18 101500").unwrap();
        assert_eq!(na_lixeira, ".trash/a 2026-11-18 101500.md");
        assert!(nomes(&c, "").is_empty());
        assert_eq!(
            fs::read_to_string(c.raiz().join(&na_lixeira)).unwrap(),
            "conteúdo"
        );
        // Pasta inteira, com o que tem dentro.
        c.criar_pasta("", "P").unwrap();
        c.salvar_nota("P/n.md", "dentro").unwrap();
        let l = c.para_a_lixeira_com("P", "2026-11-18 101501").unwrap();
        assert_eq!(
            fs::read_to_string(c.raiz().join(l).join("n.md")).unwrap(),
            "dentro"
        );
        assert!(nomes(&c, "").is_empty());
        // Duas com o mesmo nome e o mesmo carimbo não se sobrescrevem.
        c.salvar_nota("a.md", "novo").unwrap();
        let outra = c.para_a_lixeira_com("a.md", "2026-11-18 101500").unwrap();
        assert_ne!(outra, na_lixeira);
        assert_eq!(
            fs::read_to_string(c.raiz().join(&na_lixeira)).unwrap(),
            "conteúdo"
        );
    }

    #[test]
    fn lixeira_recusa_caminho_fora_do_cofre() {
        let (t, c) = cofre();
        fs::write(t.path().join("fora.md"), "x").unwrap();
        assert!(c.para_a_lixeira_com("../fora.md", "c").is_err());
        assert!(t.path().join("fora.md").exists());
    }

    #[test]
    fn explicar_traduz() {
        assert!(explicar(&io::Error::from(io::ErrorKind::NotFound)).contains("Não encontrei"));
        assert!(explicar(&io::Error::from(io::ErrorKind::PermissionDenied)).contains("Android"));
    }
}
