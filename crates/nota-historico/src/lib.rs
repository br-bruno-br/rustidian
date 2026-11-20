//! Histórico completo das notas (fundação H1): versões guardadas em pouco espaço e abertas na hora.
//! Ver `docs/bruno/HISTORICO-NOTAS.md` e o `README.md` deste crate.
#![forbid(unsafe_code)]

pub mod agrupador;
pub mod codec;
pub mod desfazer;
pub mod diff;
pub mod erro;
pub mod escritor;
pub mod historico;
pub mod pedacos;
pub mod registro;
pub mod retencao;

pub use agrupador::{Agrupador, Evento, Motivo, Ritmo};
pub use codec::{Codec, ZstdCodec};
pub use desfazer::{
    AberturaDesfazer, ArvoreArquivada, Galho, Galhos, LimitesDesfazer, NoId, Registrado,
    resumo_da_mudanca,
};
pub use diff::{Tipo, Trecho, diff_palavras};
pub use erro::{Erro, Result};
pub use escritor::{
    Apagado, MudancaDoTrecho, achar_apagados, dobrar, faixa_de_antes, filtrar_apagados,
    historico_do_trecho, normalizar,
};
pub use historico::{
    ApagadoEmTodas, Durabilidade, Estatisticas, EstatisticasNota, Historico, Opcoes,
    RelatorioCompactacao, RelatorioPoda,
};
pub use pedacos::ParametrosPedacos;
pub use registro::{Impressao, NotaId, Origem, VersaoId, VersaoInfo, impressao_do_texto};
pub use retencao::PoliticaRetencao;
