//! `RemotoHttp`: o cliente HTTP do servidor `nota-servidor`.
//!
//! - **ureq 3 sem TLS** (`default-features = false`): é só HTTP puro, sem OpenSSL, sem rustls e
//!   sem runtime async. O servidor só escuta no IP da Tailscale, que já cifra o caminho (WireGuard),
//!   e o conteúdo das notas já vai cifrado ponta a ponta; o único segredo em claro no fio é o
//!   token do aparelho.
//! - **Só o servidor dado:** URL `http://host[:porta]`; qualquer outro esquema é recusado,
//!   redirecionamento é desligado (o token não vai para outro lugar) e o proxy do ambiente é
//!   ignorado.
//! - **Tempos limite:** 10 s para conectar e 120 s no total por chamada.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::protocolo::{
    BLOB_MAXIMO, CABECALHO_HASH, Conflito409, Mudancas, PedidoPublicar, RespostaPublicar, hex64,
    sha256_hex,
};
use crate::remoto::{ErroRemoto, Remoto};

/// Configuração do cliente: endereço do servidor, token do aparelho e o nome do aparelho.
/// A chave do cofre não entra aqui: chega ao `Sincronizador` como `ChaveCofre`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    /// `http://100.x.y.z:8443`
    pub servidor: String,
    /// Token impresso uma vez por `nota-servidor token novo <aparelho>`.
    pub token: String,
    /// Nome deste aparelho (aparece nas cópias de conflito).
    pub aparelho: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("servidor", &self.servidor)
            .field("token", &"***")
            .field("aparelho", &self.aparelho)
            .finish()
    }
}

impl Config {
    /// Confere os três campos sem tocar a rede.
    pub fn validar(&self) -> Result<(), String> {
        base_do_servidor(&self.servidor)?;
        if self.token.trim().is_empty() || self.token.chars().any(char::is_whitespace) {
            return Err("token vazio ou com espaço".into());
        }
        let a = self.aparelho.trim();
        if a.is_empty() || a.chars().count() > 64 || a.chars().any(char::is_control) {
            return Err("nome do aparelho vazio, longo demais ou com caractere de controle".into());
        }
        Ok(())
    }
}

/// `http://host[:porta][/]` → `http://host[:porta]`. Recusa HTTPS, usuário, caminho e consulta.
pub fn base_do_servidor(url: &str) -> Result<String, String> {
    let resto = url
        .trim()
        .strip_prefix("http://")
        .ok_or("o servidor tem de começar com http:// (só HTTP, pela Tailscale)")?;
    let resto = resto.strip_suffix('/').unwrap_or(resto);
    if resto.is_empty()
        || resto
            .chars()
            .any(|c| c.is_whitespace() || "/@?#\\".contains(c) || c.is_control())
    {
        return Err("endereço do servidor inválido (use http://ip:porta)".into());
    }
    Ok(format!("http://{resto}"))
}

/// Cliente HTTP do servidor.
pub struct RemotoHttp {
    agent: ureq::Agent,
    base: String,
    autorizacao: String,
}

impl fmt::Debug for RemotoHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemotoHttp")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl RemotoHttp {
    pub fn novo(config: &Config) -> Result<Self, String> {
        config.validar()?;
        Self::com_tempos(config, Duration::from_secs(10), Duration::from_secs(120))
    }

    /// Como `novo`, com tempos escolhidos (os testes usam tempos curtos).
    pub fn com_tempos(
        config: &Config,
        conectar: Duration,
        total: Duration,
    ) -> Result<Self, String> {
        config.validar()?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .timeout_connect(Some(conectar))
            .timeout_global(Some(total))
            .user_agent("nota-sync")
            .build()
            .into();
        Ok(Self {
            agent,
            base: base_do_servidor(&config.servidor)?,
            autorizacao: format!("Bearer {}", config.token.trim()),
        })
    }

    fn url(&self, caminho: &str) -> String {
        format!("{}{caminho}", self.base)
    }

    fn erro_de_status(status: u16, corpo: &[u8]) -> ErroRemoto {
        match status {
            401 | 403 => ErroRemoto::NaoAutorizado,
            404 => ErroRemoto::NaoEncontrado,
            _ => ErroRemoto::Protocolo(format!(
                "status {status}: {}",
                String::from_utf8_lossy(&corpo[..corpo.len().min(200)])
            )),
        }
    }

    fn rede(e: ureq::Error) -> ErroRemoto {
        ErroRemoto::Rede(e.to_string())
    }

    fn ler(
        resp: &mut ureq::http::Response<ureq::Body>,
        limite: usize,
    ) -> Result<Vec<u8>, ErroRemoto> {
        resp.body_mut()
            .with_config()
            .limit(limite as u64)
            .read_to_vec()
            .map_err(Self::rede)
    }
}

impl Remoto for RemotoHttp {
    fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto> {
        let mut resp = self
            .agent
            .get(self.url(&format!("/v1/mudancas?desde={desde}")))
            .header("Authorization", &self.autorizacao)
            .call()
            .map_err(Self::rede)?;
        let status = resp.status().as_u16();
        let corpo = Self::ler(&mut resp, 64 * 1024 * 1024)?;
        if status != 200 {
            return Err(Self::erro_de_status(status, &corpo));
        }
        serde_json::from_slice(&corpo)
            .map_err(|e| ErroRemoto::Protocolo(format!("mudanças ilegíveis: {e}")))
    }

    fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto> {
        if !hex64(hash) {
            return Err(ErroRemoto::Protocolo("hash inválido".into()));
        }
        let mut resp = self
            .agent
            .get(self.url(&format!("/v1/blob/{hash}")))
            .header("Authorization", &self.autorizacao)
            .call()
            .map_err(Self::rede)?;
        let status = resp.status().as_u16();
        let corpo = Self::ler(&mut resp, BLOB_MAXIMO)?;
        if status != 200 {
            return Err(Self::erro_de_status(status, &corpo));
        }
        Ok(corpo)
    }

    fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto> {
        let hash = sha256_hex(bytes);
        let mut resp = self
            .agent
            .put(self.url("/v1/blob"))
            .header("Authorization", &self.autorizacao)
            .header(CABECALHO_HASH, &hash)
            .content_type("application/octet-stream")
            .send(bytes)
            .map_err(Self::rede)?;
        let status = resp.status().as_u16();
        let corpo = Self::ler(&mut resp, 4096)?;
        if status != 200 {
            return Err(Self::erro_de_status(status, &corpo));
        }
        #[derive(Deserialize)]
        struct Resposta {
            hash: String,
        }
        let r: Resposta = serde_json::from_slice(&corpo)
            .map_err(|e| ErroRemoto::Protocolo(format!("resposta do blob ilegível: {e}")))?;
        if r.hash != hash {
            return Err(ErroRemoto::Protocolo(
                "o servidor devolveu um hash diferente do enviado".into(),
            ));
        }
        Ok(hash)
    }

    fn publicar(
        &mut self,
        id: &str,
        versao_base: u64,
        hash: &str,
        apagado: bool,
    ) -> Result<Result<u64, Conflito409>, ErroRemoto> {
        if !hex64(id) || (!apagado && !hex64(hash)) {
            return Err(ErroRemoto::Protocolo("id ou hash inválido".into()));
        }
        let pedido = PedidoPublicar {
            versao_base,
            hash_blob: (!apagado).then(|| hash.to_string()),
            apagado,
        };
        let json = serde_json::to_vec(&pedido).expect("pedido serializa");
        let mut resp = self
            .agent
            .post(self.url(&format!("/v1/item/{id}")))
            .header("Authorization", &self.autorizacao)
            .content_type("application/json")
            .send(&json[..])
            .map_err(Self::rede)?;
        let status = resp.status().as_u16();
        let corpo = Self::ler(&mut resp, 1024 * 1024)?;
        match status {
            200 => {
                let r: RespostaPublicar = serde_json::from_slice(&corpo)
                    .map_err(|e| ErroRemoto::Protocolo(format!("resposta ilegível: {e}")))?;
                Ok(Ok(r.versao))
            }
            409 => {
                let c: Conflito409 = serde_json::from_slice(&corpo)
                    .map_err(|e| ErroRemoto::Protocolo(format!("409 ilegível: {e}")))?;
                Ok(Err(c))
            }
            _ => Err(Self::erro_de_status(status, &corpo)),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn cfg(servidor: &str) -> Config {
        Config {
            servidor: servidor.into(),
            token: "abc".into(),
            aparelho: "pc".into(),
        }
    }

    #[test]
    fn so_aceita_http_simples() {
        assert_eq!(
            base_do_servidor("http://100.1.2.3:8443/").unwrap(),
            "http://100.1.2.3:8443"
        );
        for ruim in [
            "https://x:1",
            "ftp://x",
            "x:1",
            "http://",
            "http://u:p@x:1",
            "http://x:1/v1",
            "http://x y",
            "http://x:1?a=b",
        ] {
            assert!(base_do_servidor(ruim).is_err(), "{ruim}");
        }
    }

    #[test]
    fn config_valida_e_esconde_o_token() {
        assert!(cfg("http://127.0.0.1:1").validar().is_ok());
        let mut c = cfg("http://127.0.0.1:1");
        c.token = "a b".into();
        assert!(c.validar().is_err());
        c.token = "segredo".into();
        c.aparelho = String::new();
        assert!(c.validar().is_err());
        c.aparelho = "pc".into();
        assert!(!format!("{c:?}").contains("segredo"));
    }

    #[test]
    fn servidor_fora_do_ar_e_erro_de_rede() {
        // porta 1 local: ninguém escuta
        let mut r = RemotoHttp::com_tempos(
            &cfg("http://127.0.0.1:1"),
            Duration::from_millis(500),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(matches!(r.mudancas(0), Err(ErroRemoto::Rede(_))));
    }
}
