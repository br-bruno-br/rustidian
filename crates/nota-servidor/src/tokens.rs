//! Tokens por aparelho. O servidor guarda só o SHA-256 de cada token em `dados/tokens`
//! (uma linha `aparelho<TAB>sha256`) e compara em tempo constante.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use nota_sync::protocolo::{hex64, sha256_hex};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub fn arquivo(dados: &Path) -> PathBuf {
    dados.join("tokens")
}

/// Um aparelho e o SHA-256 (32 bytes) do seu token.
#[derive(Debug, Clone)]
pub struct Entrada {
    pub aparelho: String,
    pub hash: [u8; 32],
}

fn de_hex(s: &str) -> Option<[u8; 32]> {
    if !hex64(s) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, par) in s.as_bytes().chunks(2).enumerate() {
        let hi = (par[0] as char).to_digit(16)?;
        let lo = (par[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

/// Lê o arquivo de tokens. Arquivo ausente = nenhum token (o servidor recusa tudo).
/// Linha mal formada é ignorada.
pub fn carregar(dados: &Path) -> Vec<Entrada> {
    let Ok(texto) = fs::read_to_string(arquivo(dados)) else {
        return Vec::new();
    };
    texto
        .lines()
        .filter_map(|l| {
            let (nome, hash) = l.split_once('\t')?;
            Some(Entrada {
                aparelho: nome.to_string(),
                hash: de_hex(hash.trim())?,
            })
        })
        .collect()
}

/// Confere o token apresentado contra **todas** as entradas, sem parar na primeira que casa e
/// com comparação em tempo constante. Devolve o nome do aparelho.
pub fn conferir(entradas: &[Entrada], apresentado: &str) -> Option<String> {
    let h: [u8; 32] = Sha256::digest(apresentado.as_bytes()).into();
    let mut achado: Option<&Entrada> = None;
    for e in entradas {
        if bool::from(e.hash.ct_eq(&h)) {
            achado = Some(e);
        }
    }
    achado.map(|e| e.aparelho.clone())
}

/// Cria um token novo para `aparelho` e devolve o token em claro (única vez em que existe).
pub fn novo(dados: &Path, aparelho: &str) -> io::Result<String> {
    let nome = aparelho.trim();
    if nome.is_empty()
        || nome.chars().count() > 64
        || nome.chars().any(|c| c.is_control() || c == '\t')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nome do aparelho vazio, longo demais ou com caractere de controle",
        ));
    }
    fs::create_dir_all(dados)?;
    if carregar(dados).iter().any(|e| e.aparelho == nome) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "já existe um token para esse aparelho (apague a linha de dados/tokens para trocar)",
        ));
    }
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = format!(
        "nst_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let linha = format!("{nome}\t{}\n", sha256_hex(token.as_bytes()));
    let mut opcoes = fs::OpenOptions::new();
    opcoes.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opcoes.mode(0o600);
    }
    let mut f = opcoes.open(arquivo(dados))?;
    f.write_all(linha.as_bytes())?;
    f.sync_all()?;
    Ok(token)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn token_novo_so_guarda_o_hash() {
        let d = tempfile::tempdir().unwrap();
        let t = novo(d.path(), "celular").unwrap();
        let arq = fs::read_to_string(arquivo(d.path())).unwrap();
        assert!(!arq.contains(&t));
        assert!(arq.contains(&sha256_hex(t.as_bytes())));
        let es = carregar(d.path());
        assert_eq!(conferir(&es, &t).as_deref(), Some("celular"));
        assert_eq!(conferir(&es, "errado"), None);
        assert_eq!(conferir(&es, ""), None);
        assert!(novo(d.path(), "celular").is_err());
        let t2 = novo(d.path(), "pc").unwrap();
        let es = carregar(d.path());
        assert_eq!(conferir(&es, &t2).as_deref(), Some("pc"));
        assert_eq!(conferir(&es, &t).as_deref(), Some("celular"));
    }

    #[test]
    fn sem_arquivo_nao_aceita_nada() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(conferir(&carregar(d.path()), "qualquer"), None);
    }
}
