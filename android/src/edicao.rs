//! A nota aberta no editor, compartilhada pelo app inteiro (o tique de 0,6 s, o Voltar do
//! Android e a ida para segundo plano precisam gravá-la de fora da tela da nota). É só uma
//! casca de sinal em volta de `logica::rascunho::Rascunho`, que tem as regras e os testes.

use std::time::Instant;

use dioxus::prelude::*;

use crate::logica::cofre::Cofre;
use crate::logica::rascunho::{Estado, Rascunho};

#[derive(Clone, Copy)]
pub struct Edicao {
    rascunho: Signal<Rascunho>,
}

impl Edicao {
    pub fn nova() -> Self {
        Self {
            rascunho: Signal::new(Rascunho::default()),
        }
    }

    pub fn abrir(mut self, caminho: &str, texto: String) {
        self.rascunho.write().abrir(caminho, texto);
    }

    pub fn digitou(mut self, texto: String) {
        self.rascunho.write().digitou(texto, Instant::now());
    }

    pub fn mudou_de_caminho(mut self, novo: &str) {
        self.rascunho.write().mudou_de_caminho(novo);
    }

    /// Grava já, se houver texto pendente (não redesenha nada quando não há).
    pub fn salvar(mut self, cofre: &Cofre) {
        if self.rascunho.peek().sujo() {
            self.rascunho.write().salvar(cofre);
        }
    }

    /// Grava se o dono parou de digitar há tempo suficiente (chamado a cada tique).
    pub fn salvar_se_vencido(mut self, cofre: &Cofre) {
        let agora = Instant::now();
        if self.rascunho.peek().vencido(agora) {
            self.rascunho.write().salvar_se_vencido(cofre, agora);
        }
    }

    /// A nota deixou de ser a aberta. Só fecha se ainda for a de `caminho` (uma nota
    /// renomeada abre outra tela antes de a antiga ser descartada).
    pub fn fechar_se(mut self, caminho: &str) {
        if self.rascunho.peek().caminho() == Some(caminho) {
            self.rascunho.write().fechar();
        }
    }

    pub fn fechar(mut self) {
        self.rascunho.write().fechar();
    }

    /// Há texto que ainda não foi para o disco?
    pub fn pendente(&self) -> bool {
        self.rascunho.peek().sujo()
    }

    /// O que o indicador mostra (quem chama passa a ser redesenhado quando mudar).
    pub fn estado(&self) -> Estado {
        self.rascunho.read().estado()
    }
}
