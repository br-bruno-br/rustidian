//! Os "serviços" do app: o que as telas usam e que vive enquanto o app está aberto.
//! O cofre aberto e a sincronização (R7b-3: configuração, ciclo numa thread e o estado que a
//! tela de Sincronizar mostra). As telas leem tudo pelo mesmo contexto
//! (`use_context::<Servicos>()`).

use std::path::PathBuf;

use crate::logica::agenda::Tempos;
use crate::logica::ajustes::Arquivos;
use crate::logica::cofre::Cofre;
use crate::logica::local::{Local, local_do_cofre, pasta_de_estado};
use crate::logica::sincronia::Sincronia;

#[derive(Clone, PartialEq, Eq)]
pub struct Servicos {
    pub cofre: Cofre,
    /// Aviso (se o cofre caiu na pasta de reserva) para a lista mostrar.
    pub aviso: Option<String>,
    /// A sincronização deste cofre.
    pub sync: Sincronia,
}

impl Servicos {
    /// Abre o cofre do aparelho. O erro vem em português, pronto para a tela.
    pub fn abrir() -> Result<Self, String> {
        Self::de_local_com_estado(local_do_cofre(), pasta_de_estado())
    }

    /// Para testes e prints: o estado da sincronização fica ao lado do cofre.
    #[cfg(test)]
    pub fn de_local(local: Local) -> Result<Self, String> {
        let estado = local.raiz.with_file_name("estado-sync");
        Self::de_local_com_estado(local, estado)
    }

    pub fn de_local_com_estado(local: Local, estado: PathBuf) -> Result<Self, String> {
        let cofre = Cofre::abrir(local.raiz.clone()).map_err(|e| {
            format!(
                "Não consegui abrir a pasta do cofre ({}): {}",
                local.raiz.display(),
                crate::logica::cofre::explicar(&e)
            )
        })?;
        let sync = Sincronia::nova(cofre.clone(), Arquivos::em(estado), Tempos::default());
        Ok(Self {
            cofre,
            aviso: local.aviso,
            sync,
        })
    }
}
