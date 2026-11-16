//! The save gate: the editor's writes and the sync cycle never touch the vault's files at the
//! same time.
//!
//! The sync engine checks a file's hash and then replaces or moves it (`nota-sync`, "Desvios da
//! R4": the window between the check and the `rename`). If the editor saved in that window, the
//! user's text could be overwritten. Every write of the editor (autosave, Ctrl+S, a new note,
//! the trash) takes this gate; the cycle takes it too.
//!
//! **The cycle does not hold the gate while it waits for the network.** `Gated` releases it
//! before every call to the server and takes it back when the call returns, so the gate is held
//! only in the stretches between two calls: the disk work (scan, check, write, move). A slow or
//! dead server (up to 120 s per call) therefore never freezes saving. The one cost is that the
//! scan also runs under the gate; it is read-only and takes milliseconds on a normal vault.

use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

use nota_sync::protocolo::{Conflito409, Mudancas};
use nota_sync::remoto::{EnvioChave, ErroRemoto, Remoto};

/// A lock with explicit `acquire`/`release`, because the sync cycle drops and retakes it from
/// inside the `Remoto` calls (a guard that lives in a scope could not).
#[derive(Default)]
pub struct SaveGate {
    busy: Mutex<bool>,
    free: Condvar,
}

/// Releases the gate when dropped.
pub struct Hold<'a>(&'a SaveGate);

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl SaveGate {
    /// Waits until nobody else holds the gate, then holds it until the guard is dropped.
    pub fn hold(&self) -> Hold<'_> {
        self.acquire();
        Hold(self)
    }

    pub fn acquire(&self) {
        let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        while *busy {
            busy = self.free.wait(busy).unwrap_or_else(PoisonError::into_inner);
        }
        *busy = true;
    }

    pub fn release(&self) {
        *self.busy.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.free.notify_one();
    }

    pub fn is_held(&self) -> bool {
        *self.busy.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The gate every editor write in this process takes.
pub fn global() -> &'static Arc<SaveGate> {
    static GATE: OnceLock<Arc<SaveGate>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(SaveGate::default()))
}

/// A `Remoto` that lets go of the gate for the duration of each call to the server.
///
/// The thread that runs the cycle calls `enter` before `Sincronizador::ciclo` and `leave` after
/// it; in between, every server call runs without the gate. Dropping it releases the gate, so a
/// panic in the middle of a cycle cannot leave the editor locked out.
pub struct Gated<R> {
    inner: R,
    gate: Arc<SaveGate>,
    held: bool,
}

impl<R> Gated<R> {
    pub fn new(inner: R, gate: Arc<SaveGate>) -> Self {
        Self {
            inner,
            gate,
            held: false,
        }
    }

    pub fn enter(&mut self) {
        if !self.held {
            self.gate.acquire();
            self.held = true;
        }
    }

    pub fn leave(&mut self) {
        if self.held {
            self.held = false;
            self.gate.release();
        }
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Runs `call` against the server with the gate released, and takes it back afterwards
    /// when this thread had it.
    fn outside<T>(&mut self, call: impl FnOnce(&mut R) -> T) -> T {
        let had = self.held;
        self.leave();
        let out = call(&mut self.inner);
        if had {
            self.enter();
        }
        out
    }
}

impl<R> Drop for Gated<R> {
    fn drop(&mut self) {
        self.leave();
    }
}

impl<R: Remoto> Remoto for Gated<R> {
    fn baixar_chave(&mut self) -> Result<Option<Vec<u8>>, ErroRemoto> {
        self.outside(|r| r.baixar_chave())
    }

    fn enviar_chave(&mut self, bytes: &[u8]) -> Result<EnvioChave, ErroRemoto> {
        self.outside(|r| r.enviar_chave(bytes))
    }

    fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto> {
        self.outside(|r| r.mudancas(desde))
    }

    fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto> {
        self.outside(|r| r.baixar_blob(hash))
    }

    fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto> {
        self.outside(|r| r.enviar_blob(bytes))
    }

    fn publicar(
        &mut self,
        id: &str,
        versao_base: u64,
        hash: &str,
        apagado: bool,
    ) -> Result<Result<u64, Conflito409>, ErroRemoto> {
        self.outside(|r| r.publicar(id, versao_base, hash, apagado))
    }
}
