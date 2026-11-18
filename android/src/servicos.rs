//! Os "serviços" do app: o que as telas usam e que vive enquanto o app está aberto.
//! Hoje é só o cofre aberto. ENCAIXE R7b-3: o sincronizador e o estado do sync (último
//! resultado, erro, aparelho) entram aqui como novos campos, e as telas leem pelo mesmo
//! contexto (`use_context::<Servicos>()`).

use crate::logica::cofre::Cofre;
use crate::logica::local::{Local, local_do_cofre};

#[derive(Clone, PartialEq, Eq)]
pub struct Servicos {
    pub cofre: Cofre,
    /// Aviso (se o cofre caiu na pasta de reserva) para a lista mostrar.
    pub aviso: Option<String>,
}

impl Servicos {
    /// Abre o cofre do aparelho. O erro vem em português, pronto para a tela.
    pub fn abrir() -> Result<Self, String> {
        Self::de_local(local_do_cofre())
    }

    pub fn de_local(local: Local) -> Result<Self, String> {
        let cofre = Cofre::abrir(local.raiz.clone()).map_err(|e| {
            format!(
                "Não consegui abrir a pasta do cofre ({}): {}",
                local.raiz.display(),
                crate::logica::cofre::explicar(&e)
            )
        })?;
        Ok(Self {
            cofre,
            aviso: local.aviso,
        })
    }
}
