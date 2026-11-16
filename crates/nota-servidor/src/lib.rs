//! Servidor de sincronização das notas. Guarda só blobs cifrados (nomeados pelo SHA-256) e
//! o índice `itens` num SQLite; não tem a chave do cofre e não lê nenhuma nota.
//! Ver docs/bruno/PLANO.md, seção "Protocolo de sync".
#![forbid(unsafe_code)]

pub mod banco;
pub mod tokens;
pub mod web;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use serde::Deserialize;

/// Como o servidor sobe.
#[derive(Debug, Clone)]
pub struct ConfigServidor {
    /// Pasta de dados: `servidor.sqlite3`, `blobs/`, `tokens`.
    pub dados: PathBuf,
    /// Onde escutar (o IP da Tailscale, `100.x.y.z:porta`).
    pub escutar: SocketAddr,
    /// Só com isto o servidor aceita `0.0.0.0` ou `::`.
    pub permitir_todas_interfaces: bool,
}

/// O que `dados/config.json` pode trazer.
#[derive(Debug, Deserialize)]
pub struct ArquivoConfig {
    pub escutar: Option<String>,
}

/// Lê `dados/config.json`, se existir.
pub fn ler_config(dados: &Path) -> Result<Option<ArquivoConfig>, String> {
    let p = dados.join("config.json");
    match std::fs::read(&p) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|e| format!("{}: {e}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// Recusa escutar em todas as interfaces sem a flag explícita: o servidor é para a Tailscale.
pub fn validar_escuta(addr: SocketAddr, permitir_todas: bool) -> Result<(), String> {
    let ip: IpAddr = addr.ip();
    if ip.is_unspecified() && !permitir_todas {
        return Err(format!(
            "recusado: {addr} escuta em todas as interfaces. Use o IP da Tailscale (100.x.y.z) \
             ou passe --permitir-todas-interfaces se for isso mesmo que você quer"
        ));
    }
    Ok(())
}

/// Um servidor em execução numa thread própria (usado pelo teste de integração e útil a
/// quem queira embutir). Para ao ser largado.
pub struct ServidorLocal {
    endereco: SocketAddr,
    parar: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ServidorLocal {
    pub fn iniciar(config: ConfigServidor) -> Result<Self, String> {
        validar_escuta(config.escutar, config.permitir_todas_interfaces)?;
        let banco = banco::Banco::abrir(&config.dados).map_err(|e| e.to_string())?;
        let estado = Arc::new(web::Estado {
            banco,
            dados: config.dados.clone(),
        });
        let (tx_addr, rx_addr) = mpsc::channel::<Result<SocketAddr, String>>();
        let (parar, rx_parar) = tokio::sync::oneshot::channel::<()>();
        let escutar = config.escutar;
        let thread = std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx_addr.send(Err(e.to_string()));
                    return;
                }
            };
            rt.block_on(async move {
                let listener = match tokio::net::TcpListener::bind(escutar).await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = tx_addr.send(Err(format!("não consegui escutar {escutar}: {e}")));
                        return;
                    }
                };
                let local = listener.local_addr().map_err(|e| e.to_string());
                let _ = tx_addr.send(local);
                let app = web::roteador(estado);
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = rx_parar.await;
                    })
                    .await;
            });
        });
        match rx_addr.recv() {
            Ok(Ok(endereco)) => Ok(Self {
                endereco,
                parar: Some(parar),
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("o servidor não subiu".into()),
        }
    }

    pub fn endereco(&self) -> SocketAddr {
        self.endereco
    }
}

impl Drop for ServidorLocal {
    fn drop(&mut self) {
        if let Some(p) = self.parar.take() {
            let _ = p.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Sobe o servidor e só volta quando ele para (Ctrl+C).
pub fn servir(config: ConfigServidor) -> Result<(), String> {
    validar_escuta(config.escutar, config.permitir_todas_interfaces)?;
    let banco = banco::Banco::abrir(&config.dados).map_err(|e| e.to_string())?;
    let estado = Arc::new(web::Estado {
        banco,
        dados: config.dados.clone(),
    });
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(config.escutar)
            .await
            .map_err(|e| format!("não consegui escutar {}: {e}", config.escutar))?;
        eprintln!("nota-servidor escutando em {}", config.escutar);
        axum::serve(listener, web::roteador(estado))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn recusa_todas_as_interfaces_sem_a_flag() {
        for a in ["0.0.0.0:8443", "[::]:8443"] {
            let addr: SocketAddr = a.parse().unwrap();
            assert!(validar_escuta(addr, false).is_err(), "{a}");
            assert!(validar_escuta(addr, true).is_ok(), "{a}");
        }
        for a in ["100.64.1.2:8443", "127.0.0.1:0"] {
            assert!(validar_escuta(a.parse().unwrap(), false).is_ok(), "{a}");
        }
    }
}
