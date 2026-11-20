//! A configuração da sincronização neste aparelho e onde ela fica.
//!
//! **Fora do cofre** (copiar ou sincronizar o cofre nunca leva a configuração junto), na pasta
//! interna do app (`/data/data/<pacote>/files/sync`), que só o próprio app enxerga:
//! - `config.json`: endereço, nome do aparelho, token e se está conectado;
//! - `estado.redb`: o banco do `nota-sync` (o que já foi sincronizado e o diário). É só cache:
//!   apagá-lo faz o próximo ciclo recomeçar do zero sem perder nota.
//!
//! **A frase-senha nunca vai para arquivo**, nem a chave derivada: ver "Decisão da frase" no
//! PLANO (R7b-3). O token fica aqui porque o Android não tem, sem JNI, um cofre de credenciais
//! como o do desktop.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use nota_sync::cofre as disco;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ajustes {
    /// `http://100.x.y.z:8443`
    pub servidor: String,
    /// Nome deste aparelho (aparece nas cópias de conflito).
    pub aparelho: String,
    /// O token do aparelho (o servidor guarda só o hash dele).
    pub token: String,
    /// A conexão foi feita e a frase já abriu a chave do cofre uma vez.
    pub conectado: bool,
}

impl fmt::Debug for Ajustes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ajustes")
            .field("servidor", &self.servidor)
            .field("aparelho", &self.aparelho)
            .field("token", &"***")
            .field("conectado", &self.conectado)
            .finish()
    }
}

/// O nome com que o aparelho começa.
pub const APARELHO_PADRAO: &str = "Celular";

/// A pasta de estado da sincronização e os dois arquivos dentro dela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arquivos {
    pasta: PathBuf,
}

impl Arquivos {
    pub fn em(pasta: PathBuf) -> Self {
        Self { pasta }
    }

    pub fn pasta(&self) -> &Path {
        &self.pasta
    }

    pub fn config(&self) -> PathBuf {
        self.pasta.join("config.json")
    }

    pub fn banco(&self) -> PathBuf {
        self.pasta.join("estado.redb")
    }

    /// Lê a configuração; sem arquivo é "nada configurado".
    pub fn carregar(&self) -> Result<Ajustes, String> {
        match fs::read(self.config()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                format!(
                    "Não consegui ler as configurações de sincronização (arquivo danificado): {e}"
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ajustes::default()),
            Err(e) => Err(format!(
                "Não consegui ler as configurações de sincronização: {e}"
            )),
        }
    }

    /// Grava de forma atômica (um arquivo pela metade nunca aparece).
    pub fn salvar(&self, ajustes: &Ajustes) -> Result<(), String> {
        let erro = |e: std::io::Error| format!("Não consegui salvar as configurações: {e}");
        fs::create_dir_all(&self.pasta).map_err(erro)?;
        let json = serde_json::to_vec_pretty(ajustes).map_err(|e| e.to_string())?;
        disco::escrever_atomico(&self.pasta, "config.json", &json).map_err(erro)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.config(), fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// O banco de estado é cache do que já foi sincronizado; uma conexão nova começa limpa
    /// (o estado antigo é de outro servidor).
    pub fn esquecer_banco(&self) {
        let _ = fs::remove_file(self.banco());
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn sem_arquivo_e_nada_configurado() {
        let t = tempfile::tempdir().unwrap();
        let a = Arquivos::em(t.path().join("sync"));
        assert_eq!(a.carregar().unwrap(), Ajustes::default());
        assert!(!a.config().exists());
    }

    #[test]
    fn ida_e_volta_e_arquivo_danificado() {
        let t = tempfile::tempdir().unwrap();
        let a = Arquivos::em(t.path().join("sync"));
        let aj = Ajustes {
            servidor: "http://100.1.2.3:8443".into(),
            aparelho: "Pixel".into(),
            token: "nst_abc".into(),
            conectado: true,
        };
        a.salvar(&aj).unwrap();
        assert_eq!(a.carregar().unwrap(), aj);
        fs::write(a.config(), b"nao e json").unwrap();
        assert!(a.carregar().is_err());
    }

    #[test]
    fn nenhum_campo_chamado_frase_e_o_token_nao_aparece_no_debug() {
        let aj = Ajustes {
            token: "segredo-do-token".into(),
            ..Ajustes::default()
        };
        let json = serde_json::to_string(&aj).unwrap();
        for proibido in ["frase", "passphrase", "chave", "key"] {
            assert!(!json.contains(proibido), "{json}");
        }
        assert!(!format!("{aj:?}").contains("segredo-do-token"));
    }

    #[test]
    fn esquecer_o_banco_nao_apaga_a_configuracao() {
        let t = tempfile::tempdir().unwrap();
        let a = Arquivos::em(t.path().join("sync"));
        a.salvar(&Ajustes::default()).unwrap();
        fs::write(a.banco(), b"x").unwrap();
        a.esquecer_banco();
        assert!(a.config().exists());
        assert!(!a.banco().exists());
    }
}
