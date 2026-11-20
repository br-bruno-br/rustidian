//! O portão de gravação: o editor (e qualquer gravação do app) e o ciclo de sincronização
//! nunca mexem nos arquivos do cofre ao mesmo tempo.
//!
//! O sincronizador confere o hash de um arquivo e só depois o troca ou o move (`nota-sync`,
//! "Desvios da R4": a janela entre conferir e o `rename`). Se o editor gravasse nessa janela, o
//! texto do dono poderia ser pisado. Toda gravação do app (salvar, nota nova, pasta nova,
//! renomear, lixeira) toma este portão; o ciclo também.
//!
//! **O ciclo não segura o portão enquanto espera a rede.** O `Gated` solta o portão antes de
//! cada chamada ao servidor (até 120 s) e o toma de volta quando ela volta; assim o portão fica
//! preso só nos trechos de disco entre duas chamadas, e um servidor fora do ar nunca congela o
//! salvamento. É o mesmo desenho do app de mesa (`src/sync/gate.rs`).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use nota_sync::protocolo::{Conflito409, Mudancas};
use nota_sync::remoto::{EnvioChave, ErroRemoto, Remoto};

/// Uma trava com `tomar`/`soltar` explícitos, porque o ciclo a solta e retoma de dentro das
/// chamadas do `Remoto` (um guarda preso a um escopo não conseguiria).
///
/// Também guarda duas coisas que as gravações do app e o sincronizador compartilham: quantas
/// gravações o app já fez (o ciclo é agendado a partir disso) e o nome deste aparelho (entra
/// no nome das cópias de conflito do editor).
pub struct Portao {
    ocupado: Mutex<bool>,
    livre: Condvar,
    gravacoes: AtomicU64,
    aparelho: Mutex<String>,
}

impl Default for Portao {
    fn default() -> Self {
        Self {
            ocupado: Mutex::new(false),
            livre: Condvar::new(),
            gravacoes: AtomicU64::new(0),
            aparelho: Mutex::new("celular".into()),
        }
    }
}

impl fmt::Debug for Portao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Portao").finish_non_exhaustive()
    }
}

/// Solta o portão quando sai de escopo.
pub struct Preso<'a>(&'a Portao);

impl Drop for Preso<'_> {
    fn drop(&mut self) {
        self.0.soltar();
    }
}

impl Portao {
    /// Espera ninguém mais estar com o portão e o segura até o guarda sair de escopo.
    pub fn prender(&self) -> Preso<'_> {
        self.tomar();
        Preso(self)
    }

    pub fn tomar(&self) {
        let mut ocupado = self.ocupado.lock().unwrap_or_else(PoisonError::into_inner);
        while *ocupado {
            ocupado = self
                .livre
                .wait(ocupado)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *ocupado = true;
    }

    pub fn soltar(&self) {
        *self.ocupado.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.livre.notify_one();
    }

    #[cfg(test)]
    pub fn esta_preso(&self) -> bool {
        *self.ocupado.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// O app gravou algo no cofre (o agendador do sync olha este contador).
    pub fn contou_gravacao(&self) {
        self.gravacoes.fetch_add(1, Ordering::SeqCst);
    }

    pub fn gravacoes(&self) -> u64 {
        self.gravacoes.load(Ordering::SeqCst)
    }

    pub fn aparelho(&self) -> String {
        self.aparelho
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn definir_aparelho(&self, nome: &str) {
        let nome = nome.trim();
        if !nome.is_empty() {
            *self.aparelho.lock().unwrap_or_else(PoisonError::into_inner) = nome.to_string();
        }
    }
}

/// O mesmo portão compartilhado por todos; é igual a outro se for o mesmo portão (assim o
/// `Cofre` continua comparável).
#[derive(Clone, Debug, Default)]
pub struct PortaoComum(pub Arc<Portao>);

impl PartialEq for PortaoComum {
    fn eq(&self, outro: &Self) -> bool {
        Arc::ptr_eq(&self.0, &outro.0)
    }
}

impl Eq for PortaoComum {}

/// Um `Remoto` que solta o portão durante cada chamada ao servidor.
///
/// A thread do ciclo chama `entrar` antes de `Sincronizador::ciclo` e `sair` depois; no meio,
/// cada chamada ao servidor roda sem o portão. Largar o `Gated` solta o portão, então um pânico
/// no meio do ciclo não deixa o editor trancado para fora.
pub struct Gated<R> {
    dentro: R,
    portao: Arc<Portao>,
    preso: bool,
}

impl<R> Gated<R> {
    pub fn novo(dentro: R, portao: Arc<Portao>) -> Self {
        Self {
            dentro,
            portao,
            preso: false,
        }
    }

    pub fn entrar(&mut self) {
        if !self.preso {
            self.portao.tomar();
            self.preso = true;
        }
    }

    pub fn sair(&mut self) {
        if self.preso {
            self.preso = false;
            self.portao.soltar();
        }
    }

    fn fora<T>(&mut self, chamada: impl FnOnce(&mut R) -> T) -> T {
        let tinha = self.preso;
        self.sair();
        let r = chamada(&mut self.dentro);
        if tinha {
            self.entrar();
        }
        r
    }
}

impl<R> Drop for Gated<R> {
    fn drop(&mut self) {
        self.sair();
    }
}

impl<R: Remoto> Remoto for Gated<R> {
    fn baixar_chave(&mut self) -> Result<Option<Vec<u8>>, ErroRemoto> {
        self.fora(|r| r.baixar_chave())
    }

    fn enviar_chave(&mut self, bytes: &[u8]) -> Result<EnvioChave, ErroRemoto> {
        self.fora(|r| r.enviar_chave(bytes))
    }

    fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto> {
        self.fora(|r| r.mudancas(desde))
    }

    fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto> {
        self.fora(|r| r.baixar_blob(hash))
    }

    fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto> {
        self.fora(|r| r.enviar_blob(bytes))
    }

    fn publicar(
        &mut self,
        id: &str,
        versao_base: u64,
        hash: &str,
        apagado: bool,
    ) -> Result<Result<u64, Conflito409>, ErroRemoto> {
        self.fora(|r| r.publicar(id, versao_base, hash, apagado))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use nota_sync::RemotoMemoria;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn so_um_de_cada_vez() {
        let p = Arc::new(Portao::default());
        let preso = p.prender();
        assert!(p.esta_preso());
        let (tx, rx) = mpsc::channel();
        let p2 = Arc::clone(&p);
        let t = std::thread::spawn(move || {
            let _g = p2.prender();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        drop(preso);
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
        t.join().unwrap();
        assert!(!p.esta_preso());
    }

    #[test]
    fn preso_no_disco_segura_e_na_rede_solta() {
        let p = Arc::new(Portao::default());
        let mut g = Gated::novo(RemotoMemoria::novo(), Arc::clone(&p));
        g.entrar();
        // Em trecho de disco: o portão está preso.
        assert!(p.esta_preso());
        // Dentro de uma chamada ao servidor: solto, e volta preso depois.
        let visto = g.fora(|_| p.esta_preso());
        assert!(!visto);
        assert!(p.esta_preso());
        g.sair();
        assert!(!p.esta_preso());
    }

    #[test]
    fn largar_o_gated_solta_o_portao() {
        let p = Arc::new(Portao::default());
        {
            let mut g = Gated::novo(RemotoMemoria::novo(), Arc::clone(&p));
            g.entrar();
            assert!(p.esta_preso());
        }
        assert!(!p.esta_preso());
    }

    #[test]
    fn contador_e_nome_do_aparelho() {
        let p = Portao::default();
        assert_eq!(p.gravacoes(), 0);
        p.contou_gravacao();
        p.contou_gravacao();
        assert_eq!(p.gravacoes(), 2);
        assert_eq!(p.aparelho(), "celular");
        p.definir_aparelho("  Pixel  ");
        assert_eq!(p.aparelho(), "Pixel");
        p.definir_aparelho("   ");
        assert_eq!(p.aparelho(), "Pixel");
    }
}
