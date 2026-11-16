//! A trait `Remoto`: tudo que o sincronizador pede ao servidor.
//!
//! **Síncrona, de propósito.** O app (egui) roda o ciclo numa thread própria; a trait síncrona
//! não obriga o app nem o Android a carregar um runtime async, e deixa a injeção de falhas
//! do `RemotoMemoria` determinística (a chamada N falha, ponto). O servidor é que é async.

use std::fmt;

pub use crate::protocolo::{Conflito409, Mudancas};

/// Falha de transporte ou resposta que não é o 409 do protocolo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroRemoto {
    /// Rede caiu, tempo esgotado, servidor fora. O ciclo para e tenta de novo depois.
    Rede(String),
    /// 401: token ausente ou errado.
    NaoAutorizado,
    /// 404 (blob que o índice cita mas o servidor não tem).
    NaoEncontrado,
    /// Qualquer outra resposta que o cliente não sabe usar (400, 413, 5xx, corpo estranho).
    Protocolo(String),
}

impl fmt::Display for ErroRemoto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErroRemoto::Rede(m) => write!(f, "rede: {m}"),
            ErroRemoto::NaoAutorizado => write!(f, "servidor recusou o token (401)"),
            ErroRemoto::NaoEncontrado => write!(f, "blob não encontrado no servidor (404)"),
            ErroRemoto::Protocolo(m) => write!(f, "resposta inesperada do servidor: {m}"),
        }
    }
}

impl std::error::Error for ErroRemoto {}

/// Resposta de `enviar_chave`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvioChave {
    /// O servidor não tinha `cofre.chave` e agora tem.
    Gravada,
    /// Já havia um `cofre.chave` (409): o servidor nunca troca.
    JaExiste,
}

/// O servidor visto pelo cliente. Nada aqui conhece conteúdo claro: só blobs cifrados.
pub trait Remoto {
    /// O `cofre.chave` do servidor (K embrulhada pela frase), ou `None` se ainda não há (404).
    fn baixar_chave(&mut self) -> Result<Option<Vec<u8>>, ErroRemoto>;

    /// Grava o `cofre.chave` **só se o servidor ainda não tem um**; com um já gravado,
    /// devolve [`EnvioChave::JaExiste`] e não muda nada.
    fn enviar_chave(&mut self, bytes: &[u8]) -> Result<EnvioChave, ErroRemoto>;

    /// Itens que mudaram depois do cursor `desde` (uma página; `mais` diz se há outra).
    fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto>;

    /// Baixa um blob pelo SHA-256 (hex).
    fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto>;

    /// Envia um blob e devolve o SHA-256 (hex) que o servidor calculou. Idempotente.
    fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto>;

    /// Publica uma versão do item `id`. `hash` vazio com `apagado = true` é uma lápide.
    /// `Ok(Ok(versao))` é a nova versão; `Ok(Err(c))` é o 409 (a versão base não é a atual).
    fn publicar(
        &mut self,
        id: &str,
        versao_base: u64,
        hash: &str,
        apagado: bool,
    ) -> Result<Result<u64, Conflito409>, ErroRemoto>;
}
