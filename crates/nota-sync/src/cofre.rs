//! Tudo que toca o disco do cofre: varredura, escrita atômica, lixeira e a checagem de
//! caminho. Nenhuma função daqui decide o que sincronizar.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::caminhos::{PREFIXO_TEMP, caminho_valido, eh_ignorado};
use crate::motor::Hash;

/// Um arquivo achado na varredura.
#[derive(Debug, Clone)]
pub struct ArquivoLocal {
    /// Caminho normalizado (NFC, `/`): o que vai para o id e para o protocolo.
    pub caminho: String,
    /// Caminho como está no disco (pode ser NFD num Linux/macOS), com `/`.
    pub real: String,
    pub hash: Hash,
}

/// Resultado da varredura.
#[derive(Debug, Default)]
pub struct Varredura {
    pub arquivos: Vec<ArquivoLocal>,
    /// O que ficou de fora e por quê (sem conteúdo de nota).
    pub recusados: Vec<String>,
}

pub fn hash_de(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

/// Temporário do sincronizador mais velho que isto é resto de uma queda.
const IDADE_TEMP_ORFAO: Duration = Duration::from_secs(10 * 60);

/// Varre o cofre. Não segue symlink, não entra em `.trash/`, `.rustidian/`, `.git/`, e
/// ignora `.obsidian/workspace*` e temporários. Nunca falha por causa de um arquivo
/// ruim: ele vai para `recusados`.
pub fn varrer(cofre: &Path) -> io::Result<Varredura> {
    let mut v = Varredura::default();
    varrer_dir(cofre, "", &mut v)?;
    v.arquivos.sort_by(|a, b| a.caminho.cmp(&b.caminho));
    Ok(v)
}

fn varrer_dir(cofre: &Path, rel_dir: &str, v: &mut Varredura) -> io::Result<()> {
    let dir = if rel_dir.is_empty() {
        cofre.to_path_buf()
    } else {
        cofre.join(rel_dir)
    };
    for entrada in fs::read_dir(&dir)? {
        let entrada = entrada?;
        let Some(nome) = entrada.file_name().to_str().map(str::to_owned) else {
            v.recusados
                .push(format!("nome que não é UTF-8 em '{rel_dir}'"));
            continue;
        };
        let real = if rel_dir.is_empty() {
            nome.clone()
        } else {
            format!("{rel_dir}/{nome}")
        };
        let tipo = entrada.file_type()?;
        if tipo.is_symlink() {
            v.recusados
                .push(format!("atalho (symlink) ignorado: {real}"));
            continue;
        }
        if tipo.is_dir() {
            // Podar a pasta inteira cedo: `.trash/`, `.rustidian/`, `.git/`.
            if rel_dir.is_empty() && eh_ignorado(&format!("{nome}/x")) {
                continue;
            }
            varrer_dir(cofre, &real, v)?;
            continue;
        }
        if !tipo.is_file() {
            continue;
        }
        if eh_temp_nosso(&nome) {
            limpar_temp_orfao(&entrada.path());
            continue;
        }
        let Ok(caminho) = nota_cripto::normalizar(&real) else {
            v.recusados.push(format!("caminho recusado: {real}"));
            continue;
        };
        if eh_ignorado(&caminho) {
            continue;
        }
        if !caminho_valido(&caminho) {
            v.recusados.push(format!(
                "nome que não cabe em todos os aparelhos: {caminho}"
            ));
            continue;
        }
        let tamanho = entrada.metadata()?.len();
        if tamanho > nota_cripto::TAMANHO_MAXIMO as u64 {
            v.recusados
                .push(format!("arquivo acima de 50 MiB: {caminho}"));
            continue;
        }
        match fs::read(entrada.path()) {
            Ok(bytes) => v.arquivos.push(ArquivoLocal {
                caminho,
                real,
                hash: hash_de(&bytes),
            }),
            Err(e) => v
                .recusados
                .push(format!("não consegui ler {caminho}: {}", e.kind())),
        }
    }
    Ok(())
}

/// Só o formato exato que `nome_temp` cria (`.nota-sync-<16 hex>.tmp`) é apagado como órfão.
/// Uma nota do dono que só comece com o prefixo nunca é apagada.
fn eh_temp_nosso(nome: &str) -> bool {
    nome.strip_prefix(PREFIXO_TEMP)
        .and_then(|r| r.strip_suffix(".tmp"))
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|c| c.is_ascii_hexdigit()))
}

fn limpar_temp_orfao(path: &Path) {
    let velho = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|idade| idade > IDADE_TEMP_ORFAO);
    if velho {
        // Temporário do próprio sincronizador (nunca uma nota).
        let _ = fs::remove_file(path);
    }
}

/// Resolve `rel` (relativo, com `/`) dentro do cofre e confere que **nenhum componente
/// existente é symlink** e que nenhum pai é arquivo. Não cria nada.
pub fn caminho_dentro(cofre: &Path, rel: &str) -> io::Result<PathBuf> {
    let ruim = |m: &str| io::Error::new(io::ErrorKind::InvalidInput, m.to_string());
    if rel.is_empty() {
        return Err(ruim("caminho vazio"));
    }
    let mut atual = cofre.to_path_buf();
    let partes: Vec<&str> = rel.split('/').collect();
    let mut existe = true;
    for (i, parte) in partes.iter().enumerate() {
        if parte.is_empty() || *parte == "." || *parte == ".." || parte.contains('\0') {
            return Err(ruim("componente inválido"));
        }
        let p = Path::new(parte);
        if p.components().count() != 1
            || !matches!(p.components().next(), Some(Component::Normal(_)))
        {
            return Err(ruim("componente inválido"));
        }
        atual.push(parte);
        if !existe {
            continue;
        }
        match fs::symlink_metadata(&atual) {
            Ok(m) => {
                if m.file_type().is_symlink() {
                    return Err(ruim("há um symlink no caminho"));
                }
                if i + 1 < partes.len() && !m.is_dir() {
                    return Err(ruim("um pai do caminho é arquivo"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => existe = false,
            Err(e) => return Err(e),
        }
    }
    if !atual.starts_with(cofre) {
        return Err(ruim("caminho fora do cofre"));
    }
    Ok(atual)
}

/// O caminho como está de fato no disco. O protocolo usa NFC, mas um Linux pode ter o arquivo
/// em NFD: sem isto, escrever o "mesmo" caminho criaria um segundo arquivo ao lado.
/// Componentes que não existem ficam como vieram.
pub fn achar_real(cofre: &Path, rel: &str) -> String {
    let partes: Vec<&str> = rel.split('/').collect();
    let mut atual = cofre.to_path_buf();
    let mut saida: Vec<String> = Vec::with_capacity(partes.len());
    for (i, p) in partes.iter().enumerate() {
        let candidato = atual.join(p);
        if fs::symlink_metadata(&candidato).is_ok() {
            saida.push((*p).to_string());
            atual = candidato;
            continue;
        }
        let achado = fs::read_dir(&atual).ok().and_then(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .find(|n| nota_cripto::normalizar(n).is_ok_and(|x| x == *p))
        });
        match achado {
            Some(n) => {
                atual = atual.join(&n);
                saida.push(n);
            }
            None => {
                saida.extend(partes[i..].iter().map(|s| (*s).to_string()));
                return saida.join("/");
            }
        }
    }
    saida.join("/")
}

/// Lê o arquivo; `None` se não existe. Recusa symlink.
pub fn ler(cofre: &Path, rel: &str) -> io::Result<Option<Vec<u8>>> {
    let p = caminho_dentro(cofre, rel)?;
    match fs::symlink_metadata(&p) {
        Ok(m) if m.is_file() => fs::read(&p).map(Some),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "o caminho não é um arquivo",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn nome_temp(dir: &Path) -> PathBuf {
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; 8];
    OsRng.fill_bytes(&mut b);
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    dir.join(format!("{PREFIXO_TEMP}{hex}.tmp"))
}

/// Cria os diretórios pai de `destino` (dentro do cofre, já checado por `caminho_dentro`).
fn criar_pais(destino: &Path) -> io::Result<()> {
    if let Some(pai) = destino.parent() {
        fs::create_dir_all(pai)?;
    }
    Ok(())
}

/// Escrita atômica: temporário no mesmo diretório, `sync_all`, rename por cima.
/// Quem chama confere antes que o arquivo não mudou desde a varredura.
pub fn escrever_atomico(cofre: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let destino = caminho_dentro(cofre, rel)?;
    criar_pais(&destino)?;
    let pai = destino.parent().unwrap_or(cofre);
    let temp = nome_temp(pai);
    let r = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temp, &destino)
    })();
    if r.is_err() {
        let _ = fs::remove_file(&temp); // o temporário é nosso
    }
    #[cfg(unix)]
    if r.is_ok()
        && let Ok(d) = fs::File::open(pai)
    {
        let _ = d.sync_all();
    }
    r
}

/// Grava `bytes` num nome que não existe ainda. Se o nome existe com **o mesmo conteúdo**, não
/// grava nada (repetir a operação depois de uma queda não duplica). Se existe com conteúdo
/// diferente, tenta `nome 2`, `nome 3`... Devolve o caminho em que o conteúdo ficou.
pub fn gravar_unico(cofre: &Path, rel: &str, bytes: &[u8]) -> io::Result<String> {
    for n in 1..=1000u32 {
        let candidato = if n == 1 {
            rel.to_string()
        } else {
            com_sufixo(rel, n)
        };
        if !caminho_valido(&candidato) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "nome de conflito inválido",
            ));
        }
        match ler(cofre, &candidato)? {
            None => {
                escrever_atomico(cofre, &candidato, bytes)?;
                return Ok(candidato);
            }
            Some(existente) if existente == bytes => return Ok(candidato),
            Some(_) => {}
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "nomes de conflito esgotados",
    ))
}

fn com_sufixo(rel: &str, n: u32) -> String {
    let (dir, arq) = match rel.rfind('/') {
        Some(i) => (&rel[..=i], &rel[i + 1..]),
        None => ("", rel),
    };
    match arq.rfind('.') {
        Some(i) if i > 0 => format!("{dir}{} {n}{}", &arq[..i], &arq[i..]),
        _ => format!("{dir}{arq} {n}"),
    }
}

/// Move o arquivo para `.trash/` do cofre, mantendo a pasta e acrescentando o carimbo ao nome.
/// Nunca apaga: é `rename`. Devolve o caminho dentro da lixeira.
pub fn mover_para_lixeira(cofre: &Path, rel: &str, carimbo: &str) -> io::Result<String> {
    let origem = caminho_dentro(cofre, rel)?;
    let destino_rel_base = {
        let (dir, arq) = match rel.rfind('/') {
            Some(i) => (&rel[..=i], &rel[i + 1..]),
            None => ("", rel),
        };
        match arq.rfind('.') {
            Some(i) if i > 0 => format!(".trash/{dir}{} {carimbo}{}", &arq[..i], &arq[i..]),
            _ => format!(".trash/{dir}{arq} {carimbo}"),
        }
    };
    for n in 1..=1000u32 {
        let alvo_rel = if n == 1 {
            destino_rel_base.clone()
        } else {
            com_sufixo(&destino_rel_base, n)
        };
        let alvo = caminho_dentro(cofre, &alvo_rel)?;
        if fs::symlink_metadata(&alvo).is_ok() {
            continue;
        }
        criar_pais(&alvo)?;
        fs::rename(&origem, &alvo)?;
        return Ok(alvo_rel);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "nomes na lixeira esgotados",
    ))
}

/// Todos os arquivos vivos do cofre (para testes e diagnóstico): caminho → conteúdo.
pub fn listar_vivos(cofre: &Path) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let v = varrer(cofre)?;
    let mut m = BTreeMap::new();
    for a in v.arquivos {
        let bytes = fs::read(cofre.join(&a.real))?;
        m.insert(a.caminho, bytes);
    }
    Ok(m)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn varredura_ignora_o_que_deve() {
        let d = dir();
        let c = d.path();
        for (p, t) in [
            ("a.md", "1"),
            ("pasta/b.md", "2"),
            (".trash/c.md", "x"),
            (".rustidian/s", "x"),
            (".obsidian/workspace.json", "x"),
            (".obsidian/app.json", "3"),
            ("d.md.tmp", "x"),
            ("e.md~", "x"),
        ] {
            let alvo = c.join(p);
            fs::create_dir_all(alvo.parent().unwrap()).unwrap();
            fs::write(alvo, t).unwrap();
        }
        let v = varrer(c).unwrap();
        let nomes: Vec<&str> = v.arquivos.iter().map(|a| a.caminho.as_str()).collect();
        assert_eq!(nomes, vec![".obsidian/app.json", "a.md", "pasta/b.md"]);
    }

    #[test]
    fn caminho_dentro_recusa_fuga_e_pai_arquivo() {
        let d = dir();
        let c = d.path();
        fs::write(c.join("arq"), "x").unwrap();
        assert!(caminho_dentro(c, "../x").is_err());
        assert!(caminho_dentro(c, "a/../../x").is_err());
        assert!(caminho_dentro(c, "arq/filho.md").is_err());
        assert!(caminho_dentro(c, "novo/filho.md").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn caminho_dentro_recusa_symlink() {
        let d = dir();
        let fora = dir();
        let c = d.path();
        std::os::unix::fs::symlink(fora.path(), c.join("atalho")).unwrap();
        assert!(caminho_dentro(c, "atalho/x.md").is_err());
        assert!(escrever_atomico(c, "atalho/x.md", b"y").is_err());
        assert!(fs::read_dir(fora.path()).unwrap().next().is_none());
    }

    #[test]
    fn escrita_atomica_substitui_e_nao_deixa_temporario() {
        let d = dir();
        let c = d.path();
        escrever_atomico(c, "p/a.md", b"um").unwrap();
        escrever_atomico(c, "p/a.md", b"dois").unwrap();
        assert_eq!(fs::read(c.join("p/a.md")).unwrap(), b"dois");
        let restos: Vec<_> = fs::read_dir(c.join("p")).unwrap().collect();
        assert_eq!(restos.len(), 1);
    }

    #[test]
    fn gravar_unico_nao_duplica_e_nao_sobrescreve() {
        let d = dir();
        let c = d.path();
        assert_eq!(gravar_unico(c, "a (c).md", b"1").unwrap(), "a (c).md");
        assert_eq!(gravar_unico(c, "a (c).md", b"1").unwrap(), "a (c).md");
        assert_eq!(gravar_unico(c, "a (c).md", b"2").unwrap(), "a (c) 2.md");
        assert_eq!(fs::read(c.join("a (c).md")).unwrap(), b"1");
    }

    #[test]
    fn lixeira_move_e_nao_apaga() {
        let d = dir();
        let c = d.path();
        escrever_atomico(c, "p/a.md", b"um").unwrap();
        let alvo = mover_para_lixeira(c, "p/a.md", "2026-11-15 143000").unwrap();
        assert_eq!(alvo, ".trash/p/a 2026-11-15 143000.md");
        assert!(!c.join("p/a.md").exists());
        assert_eq!(fs::read(c.join(&alvo)).unwrap(), b"um");
        // o mesmo nome de novo não sobrescreve a lixeira
        escrever_atomico(c, "p/a.md", b"dois").unwrap();
        let alvo2 = mover_para_lixeira(c, "p/a.md", "2026-11-15 143000").unwrap();
        assert_ne!(alvo, alvo2);
        assert_eq!(fs::read(c.join(&alvo)).unwrap(), b"um");
        assert_eq!(fs::read(c.join(&alvo2)).unwrap(), b"dois");
    }

    #[test]
    fn so_o_temporario_exato_e_reconhecido_como_nosso() {
        assert!(super::eh_temp_nosso(".nota-sync-0123456789abcdef.tmp"));
        assert!(!super::eh_temp_nosso(".nota-sync-ideias.md"));
        assert!(!super::eh_temp_nosso(".nota-sync-0123456789abcdef.md"));
        assert!(!super::eh_temp_nosso(".nota-sync-xyz.tmp"));
    }
}
