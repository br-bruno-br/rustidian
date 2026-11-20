//! A ponte entre o estado da sincronização (que muda numa thread) e os componentes: a cada
//! tique do app (a ponte já tem um de 0,6 s) comparamos o número de versão do estado com o que a
//! tela viu e, se mudou, sobe um sinal. As telas que leem `versao()` se redesenham; quem não lê
//! não paga nada.

use dioxus::prelude::*;

use crate::logica::sincronia::Sincronia;

#[derive(Clone, Copy)]
pub struct VistaSync {
    versao: Signal<u64>,
    cofre: Signal<u64>,
}

/// O que mudou desde o tique anterior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mudanca {
    /// O estado da sincronização mudou (a tela de Sincronizar e o rótulo da lista se refazem).
    pub estado: bool,
    /// Um ciclo deixou os arquivos do cofre diferentes (a lista relê; editor e leitura conferem).
    pub cofre: bool,
}

impl VistaSync {
    pub fn nova() -> Self {
        Self {
            versao: Signal::new(0),
            cofre: Signal::new(0),
        }
    }

    pub fn atualizar(mut self, sync: &Sincronia) -> Mudanca {
        let mut m = Mudanca::default();
        let v = sync.versao();
        if v != *self.versao.peek() {
            let cofre = sync.estado().cofre_mudou;
            self.versao.set(v);
            m.estado = true;
            if cofre != *self.cofre.peek() {
                self.cofre.set(cofre);
                m.cofre = true;
            }
        }
        m
    }

    /// Ler isto dentro de um componente o assina: ele se refaz quando o estado muda.
    pub fn versao(&self) -> u64 {
        *self.versao.read()
    }
}
