//! Tipos e regras do protocolo compartilhados pelo cliente (`nota-sync`) e pelo servidor
//! (`nota-servidor`). Ver docs/bruno/PLANO.md, seção "Protocolo de sync".

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maior blob aceito por servidor e cliente: conteúdo máximo mais a folga que a `nota-cripto`
/// documenta (versão, nonce, tag e o caminho cifrado dentro do blob; ver "Desvios da R3").
pub const BLOB_MAXIMO: usize = nota_cripto::TAMANHO_MAXIMO + 8 * 1024;

/// Maior `cofre.chave` aceito por `PUT /v1/chave` (o arquivo v1 tem 101 bytes).
pub const CHAVE_MAXIMA: usize = 4 * 1024;

/// Itens por página de `GET /v1/mudancas`.
pub const PAGINA_MUDANCAS: usize = 1000;

/// Um item do índice do servidor: o que o servidor sabe sobre um id de arquivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRemoto {
    pub id: String,
    pub versao: u64,
    /// `None` quando `apagado` (lápide).
    pub hash_blob: Option<String>,
    pub apagado: bool,
    pub seq: u64,
}

/// Resposta de `GET /v1/mudancas?desde=<seq>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mudancas {
    pub itens: Vec<ItemRemoto>,
    /// Cursor a usar na próxima chamada: o maior `seq` entregue (ou `desde`, se nada mudou).
    pub seq: u64,
    /// `true` se há mais itens depois deste cursor.
    pub mais: bool,
}

/// Corpo de `POST /v1/item/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PedidoPublicar {
    pub versao_base: u64,
    pub hash_blob: Option<String>,
    pub apagado: bool,
}

/// Resposta 200 de `POST /v1/item/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaPublicar {
    pub versao: u64,
    pub seq: u64,
}

/// Resposta 409: a versão base não é a atual. Traz o estado atual do item, para o cliente
/// decidir sem precisar de outra chamada (e para reconhecer a própria publicação repetida).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflito409 {
    pub versao_atual: u64,
    pub hash_blob: Option<String>,
    pub apagado: bool,
}

/// Cabeçalho opcional de `PUT /v1/blob` com o SHA-256 que o cliente espera.
pub const CABECALHO_HASH: &str = "x-hash-sha256";

/// SHA-256 em hex minúsculo (64 caracteres).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        s.push(char::from_digit(u32::from(b >> 4), 16).expect("nibble"));
        s.push(char::from_digit(u32::from(b & 15), 16).expect("nibble"));
    }
    s
}

/// `true` se `s` é um hash/id em hex minúsculo de 64 caracteres. Usado antes de montar
/// URLs e nomes de arquivo, para nada vindo de fora virar caminho.
pub fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn sha256_conhecido() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hex64_so_aceita_hex_minusculo_de_64() {
        assert!(hex64(&sha256_hex(b"x")));
        assert!(!hex64("../../etc/passwd"));
        assert!(!hex64(&"A".repeat(64)));
        assert!(!hex64(&"a".repeat(63)));
        assert!(!hex64(&"g".repeat(64)));
    }
}
