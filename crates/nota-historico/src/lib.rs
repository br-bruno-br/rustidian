//! Histórico completo das notas (fundação H1): versões guardadas em pouco espaço e abertas na hora.
//! Ver `docs/bruno/HISTORICO-NOTAS.md` e o `README.md` deste crate.
#![forbid(unsafe_code)]

pub mod agrupador;
pub mod codec;
pub mod diff;
pub mod erro;
pub mod historico;
pub mod pedacos;
pub mod registro;
pub mod retencao;

pub use agrupador::{Agrupador, Evento, Motivo, Ritmo};
pub use codec::{Codec, ZstdCodec};
pub use diff::{Tipo, Trecho, diff_palavras};
pub use erro::{Erro, Result};
pub use historico::{
    Durabilidade, Estatisticas, EstatisticasNota, Historico, Opcoes, RelatorioCompactacao,
    RelatorioPoda,
};
pub use pedacos::ParametrosPedacos;
pub use registro::{Impressao, NotaId, Origem, VersaoId, VersaoInfo, impressao_do_texto};
pub use retencao::PoliticaRetencao;
