//! Sincronização das notas entre aparelhos, com um servidor que nunca lê as notas.
//! Ver docs/bruno/PLANO.md, seção "Protocolo de sync".
//!
//! Divisão de trabalho (quem decide não faz efeito, quem faz efeito não decide):
//! - [`motor`]: função pura que compara local, base e remoto e devolve [`motor::Acao`]s;
//! - [`sincronizador`]: varre o cofre, chama o motor, grava a intenção no diário e aplica;
//! - [`remoto`]: a trait do servidor, com [`memoria::RemotoMemoria`] (testes) e
//!   [`http::RemotoHttp`] (de verdade);
//! - [`protocolo`]: tipos e limites compartilhados com o `nota-servidor`.
#![forbid(unsafe_code)]

pub mod caminhos;
pub mod http;
pub mod memoria;
pub mod motor;
pub mod protocolo;
pub mod remoto;

pub use http::{Config, RemotoHttp};
pub use memoria::RemotoMemoria;
pub use remoto::{ErroRemoto, Remoto};
