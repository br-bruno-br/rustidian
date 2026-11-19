//! Erros do histórico.

use std::fmt;

/// Tudo que pode dar errado ao ler ou gravar o histórico.
#[derive(Debug)]
pub enum Erro {
    /// Falha do banco `redb` (abrir, transação, disco cheio...).
    Banco(String),
    /// O codec (compressão) recusou os dados.
    Codec(String),
    /// Os dados guardados não fazem sentido (registro truncado, pedaço faltando).
    Corrompido(String),
    /// A versão não passou na conferência blake3: o texto lido não é o que foi gravado.
    Integridade { nota: String, versao: u64 },
    /// A nota ou a versão pedida não existe (ou foi podada).
    NaoEncontrada(String),
    /// Argumento inválido (por exemplo, trecho vazio em `apagar_texto_do_historico`).
    Argumento(String),
}

pub type Result<T> = std::result::Result<T, Erro>;

impl fmt::Display for Erro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Erro::Banco(m) => write!(f, "banco do histórico: {m}"),
            Erro::Codec(m) => write!(f, "compressão do histórico: {m}"),
            Erro::Corrompido(m) => write!(f, "histórico corrompido: {m}"),
            Erro::Integridade { nota, versao } => write!(
                f,
                "versão {versao} da nota {nota}: a impressão blake3 não confere"
            ),
            Erro::NaoEncontrada(m) => write!(f, "não encontrado: {m}"),
            Erro::Argumento(m) => write!(f, "argumento inválido: {m}"),
        }
    }
}

impl std::error::Error for Erro {}

macro_rules! de_redb {
    ($($t:ty),* $(,)?) => {$(
        impl From<$t> for Erro {
            fn from(e: $t) -> Erro {
                Erro::Banco(e.to_string())
            }
        }
    )*};
}

de_redb!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::CompactionError,
    redb::SetDurabilityError,
);

impl From<std::io::Error> for Erro {
    fn from(e: std::io::Error) -> Erro {
        Erro::Banco(e.to_string())
    }
}
