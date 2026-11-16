//! Erros do sincronizador.

use std::fmt;

use crate::remoto::ErroRemoto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroSync {
    /// O servidor falhou ou a rede caiu. O diário guarda o que estava no meio.
    Remoto(ErroRemoto),
    /// Falha de disco no cofre.
    Io(String),
    /// Falha no banco de estado (redb).
    Estado(String),
    /// Muitas exclusões de uma vez: parece cofre vazio ou desmontado. Nada foi enviado.
    ExclusaoEmMassa { lapides: usize, total: usize },
    /// Queda simulada (só para testes): o estado em disco ficou como numa queda de verdade.
    Interrompido,
}

impl fmt::Display for ErroSync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErroSync::Remoto(e) => write!(f, "{e}"),
            ErroSync::Io(m) => write!(f, "disco: {m}"),
            ErroSync::Estado(m) => write!(f, "estado local: {m}"),
            ErroSync::ExclusaoEmMassa { lapides, total } => write!(
                f,
                "ia apagar {lapides} de {total} arquivos no servidor; o cofre parece vazio ou desmontado, nada foi enviado"
            ),
            ErroSync::Interrompido => write!(f, "interrompido (teste)"),
        }
    }
}

impl std::error::Error for ErroSync {}

impl From<ErroRemoto> for ErroSync {
    fn from(e: ErroRemoto) -> Self {
        ErroSync::Remoto(e)
    }
}

impl From<std::io::Error> for ErroSync {
    fn from(e: std::io::Error) -> Self {
        ErroSync::Io(format!("{}: {e}", e.kind()))
    }
}
