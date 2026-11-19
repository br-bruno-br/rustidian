//! Histórico completo das notas (fundação H1): versões guardadas em pouco espaço e abertas na hora.
//! Ver `docs/bruno/HISTORICO-NOTAS.md` e o `README.md` deste crate.
#![forbid(unsafe_code)]

pub mod codec;
pub mod erro;
pub mod pedacos;
pub mod registro;
pub mod retencao;

pub use codec::{Codec, ZstdCodec};
pub use erro::{Erro, Result};
