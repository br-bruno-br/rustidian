//! Connecting a device: the first one creates the vault key K and puts it, wrapped by the
//! passphrase, on the server (`cofre.chave`); the others download it and unwrap it with the
//! same passphrase. Then K and the token go to the system credential store.
//!
//! The passphrase is only an argument here: it is never stored and never logged.

use std::fmt;
use std::path::Path;

use nota_cripto::{ChaveCofre, Erro as ErroCripto};
use nota_sync::remoto::{EnvioChave, ErroRemoto, Remoto};
use nota_sync::{Config, RemotoHttp};

use super::config::{SyncConfig, SyncFiles};
use super::secrets::{Secret, SecretStore, account};

/// A new vault key needs a passphrase worth the Argon2id that protects it.
pub const MIN_NEW_PASSPHRASE_CHARS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server had no `cofre.chave`: this device made the key.
    Created,
    /// The server had one and the passphrase opened it.
    Joined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// Address, device name or token is not usable (nothing was sent).
    Settings(String),
    /// The passphrase is empty, or too short to start a new vault.
    Passphrase(String),
    /// The server has a key and this passphrase does not open it.
    WrongPassphrase,
    /// The server refused the token.
    Unauthorized,
    /// The server could not be reached or answered something unexpected.
    Server(String),
    /// The server's `cofre.chave` is damaged or from a newer version.
    BadKeyFile(String),
    /// The system credential store or the settings file failed.
    Local(String),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::Settings(m) | ConnectError::Passphrase(m) | ConnectError::Local(m) => {
                write!(f, "{m}")
            }
            ConnectError::WrongPassphrase => write!(
                f,
                "Frase secreta incorreta: ela não abre a chave do cofre deste servidor. Digite a escolhida no primeiro dispositivo."
            ),
            ConnectError::Unauthorized => write!(
                f,
                "O servidor recusou o token. Confira se é o token deste dispositivo."
            ),
            ConnectError::Server(m) => write!(
                f,
                "Não foi possível acessar o servidor de sincronização: {m}"
            ),
            ConnectError::BadKeyFile(m) => {
                write!(
                    f,
                    "O arquivo da chave do cofre no servidor não pode ser usado: {m}"
                )
            }
        }
    }
}

impl std::error::Error for ConnectError {}

fn remote_error(e: ErroRemoto) -> ConnectError {
    match e {
        ErroRemoto::NaoAutorizado => ConnectError::Unauthorized,
        other => ConnectError::Server(other.to_string()),
    }
}

fn unwrap_key(wrapped: &[u8], passphrase: &str) -> Result<ChaveCofre, ConnectError> {
    ChaveCofre::desembrulhar(wrapped, passphrase).map_err(|e| match e {
        ErroCripto::FraseErrada => ConnectError::WrongPassphrase,
        other => ConnectError::BadKeyFile(other.to_string()),
    })
}

/// The heart of "first device or not". Pure with respect to the disk: talks only to `remote`.
pub fn establish_key<R: Remoto>(
    remote: &mut R,
    passphrase: &str,
) -> Result<(ChaveCofre, Outcome), ConnectError> {
    if passphrase.is_empty() {
        return Err(ConnectError::Passphrase("Digite a frase secreta.".into()));
    }
    if let Some(wrapped) = remote.baixar_chave().map_err(remote_error)? {
        return Ok((unwrap_key(&wrapped, passphrase)?, Outcome::Joined));
    }
    if passphrase.chars().count() < MIN_NEW_PASSPHRASE_CHARS {
        return Err(ConnectError::Passphrase(format!(
            "O servidor ainda não tem chave do cofre, então esta frase secreta vai proteger uma nova: use pelo menos {MIN_NEW_PASSPHRASE_CHARS} caracteres."
        )));
    }
    let key = ChaveCofre::nova();
    let wrapped = key
        .embrulhar(passphrase)
        .map_err(|e| ConnectError::BadKeyFile(e.to_string()))?;
    match remote.enviar_chave(&wrapped).map_err(remote_error)? {
        EnvioChave::Gravada => Ok((key, Outcome::Created)),
        // Another device got there first (two first devices at once): use its key, not ours.
        EnvioChave::JaExiste => {
            let theirs = remote
                .baixar_chave()
                .map_err(remote_error)?
                .ok_or_else(|| ConnectError::Server("a chave do cofre sumiu".into()))?;
            Ok((unwrap_key(&theirs, passphrase)?, Outcome::Joined))
        }
    }
}

/// What the user typed in Settings.
pub struct Credentials {
    pub server: String,
    pub device: String,
    pub token: String,
    pub passphrase: String,
}

/// Connect `vault` to the server: set up the key, then store K and the token in `store` and
/// the plain settings in `files`. Nothing is written anywhere unless the key was established.
pub fn connect_vault(
    files: &SyncFiles,
    store: &dyn SecretStore,
    vault: &Path,
    credentials: &Credentials,
) -> Result<Outcome, ConnectError> {
    let config = Config {
        servidor: credentials.server.trim().to_owned(),
        token: credentials.token.trim().to_owned(),
        aparelho: credentials.device.trim().to_owned(),
    };
    config.validar().map_err(ConnectError::Settings)?;
    let mut remote = RemotoHttp::novo(&config).map_err(ConnectError::Settings)?;
    let (key, outcome) = establish_key(&mut remote, &credentials.passphrase)?;

    store
        .set(&account(vault, Secret::Key), &key.bytes()[..])
        .map_err(ConnectError::Local)?;
    store
        .set(&account(vault, Secret::Token), config.token.as_bytes())
        .map_err(ConnectError::Local)?;
    // A new connection starts from a clean sync state: the old one belongs to whatever server
    // the vault was connected to before.
    forget_sync_state(files);
    SyncConfig {
        server: nota_sync::http::base_do_servidor(&config.servidor)
            .map_err(ConnectError::Settings)?,
        device: config.aparelho,
        connected: true,
        enabled: true,
    }
    .save(&files.config)
    .map_err(ConnectError::Local)?;
    Ok(outcome)
}

/// Disconnect: delete K and the token from the credential store and stop syncing. The vault's
/// notes, the server and the settings (address, device name) stay.
pub fn disconnect_vault(
    files: &SyncFiles,
    store: &dyn SecretStore,
    vault: &Path,
) -> Result<(), String> {
    store.delete(&account(vault, Secret::Key))?;
    store.delete(&account(vault, Secret::Token))?;
    forget_sync_state(files);
    let mut config = SyncConfig::load(&files.config).unwrap_or_default();
    config.connected = false;
    config.enabled = false;
    config.save(&files.config)
}

fn forget_sync_state(files: &SyncFiles) {
    // The state database is only a cache of "what was synced"; the notes are not in it.
    let _ = std::fs::remove_file(&files.database);
}

/// K and the token of a connected vault, from the credential store.
pub fn load_secrets(store: &dyn SecretStore, vault: &Path) -> Result<(ChaveCofre, String), String> {
    let missing = || {
        "A chave de sincronização não está no cofre de credenciais do sistema. Desconecte e conecte de novo."
            .to_owned()
    };
    let key = store
        .get(&account(vault, Secret::Key))?
        .ok_or_else(missing)?;
    let token = store
        .get(&account(vault, Secret::Token))?
        .ok_or_else(missing)?;
    let key = ChaveCofre::de_bytes(&key)
        .map_err(|_| "A chave de sincronização guardada está danificada.".to_owned())?;
    let token =
        String::from_utf8(token).map_err(|_| "O token guardado está danificado.".to_owned())?;
    Ok((key, token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nota_sync::RemotoMemoria;
    use nota_sync::protocolo::{Conflito409, Mudancas};

    #[test]
    fn first_device_creates_the_key_and_the_second_joins_with_the_same_passphrase() {
        let mut first = RemotoMemoria::novo();
        let mut second = first.outro_aparelho();
        let (k1, o1) = establish_key(&mut first, "a long passphrase").unwrap();
        assert_eq!(o1, Outcome::Created);
        let (k2, o2) = establish_key(&mut second, "a long passphrase").unwrap();
        assert_eq!(o2, Outcome::Joined);
        assert_eq!(&k1.bytes()[..], &k2.bytes()[..]);
    }

    #[test]
    fn wrong_passphrase_is_refused_with_a_clear_message() {
        let mut first = RemotoMemoria::novo();
        let mut second = first.outro_aparelho();
        establish_key(&mut first, "a long passphrase").unwrap();
        let error = establish_key(&mut second, "another passphrase").unwrap_err();
        assert_eq!(error, ConnectError::WrongPassphrase);
        assert!(error.to_string().starts_with("Frase secreta incorreta"));
    }

    #[test]
    fn short_or_empty_passphrases_are_refused_before_any_key_is_sent() {
        let mut remote = RemotoMemoria::novo();
        assert!(matches!(
            establish_key(&mut remote, ""),
            Err(ConnectError::Passphrase(_))
        ));
        assert!(matches!(
            establish_key(&mut remote, "short"),
            Err(ConnectError::Passphrase(_))
        ));
        assert_eq!(remote.baixar_chave().unwrap(), None);
    }

    /// A server that says "no key yet" and then has one by the time this device uploads.
    struct LosesTheRace {
        server: RemotoMemoria,
        rival: RemotoMemoria,
        first_download: bool,
    }

    impl Remoto for LosesTheRace {
        fn baixar_chave(&mut self) -> Result<Option<Vec<u8>>, ErroRemoto> {
            if self.first_download {
                self.first_download = false;
                return Ok(None);
            }
            self.server.baixar_chave()
        }
        fn enviar_chave(&mut self, bytes: &[u8]) -> Result<EnvioChave, ErroRemoto> {
            let rival_key = ChaveCofre::nova().embrulhar("a long passphrase").unwrap();
            self.rival.enviar_chave(&rival_key).unwrap();
            self.server.enviar_chave(bytes)
        }
        fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto> {
            self.server.mudancas(desde)
        }
        fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto> {
            self.server.baixar_blob(hash)
        }
        fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto> {
            self.server.enviar_blob(bytes)
        }
        fn publicar(
            &mut self,
            id: &str,
            base: u64,
            hash: &str,
            apagado: bool,
        ) -> Result<Result<u64, Conflito409>, ErroRemoto> {
            self.server.publicar(id, base, hash, apagado)
        }
    }

    #[test]
    fn two_first_devices_at_once_end_up_with_the_same_key() {
        let server = RemotoMemoria::novo();
        let rival = server.outro_aparelho();
        let mut remote = LosesTheRace {
            server,
            rival,
            first_download: true,
        };
        let (key, outcome) = establish_key(&mut remote, "a long passphrase").unwrap();
        assert_eq!(outcome, Outcome::Joined);
        let theirs = remote.server.baixar_chave().unwrap().unwrap();
        let opened = ChaveCofre::desembrulhar(&theirs, "a long passphrase").unwrap();
        assert_eq!(&key.bytes()[..], &opened.bytes()[..]);
    }
}
