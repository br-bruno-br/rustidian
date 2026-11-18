//! As telas do app e a navegação entre elas (o único lugar que decide "onde estou").
//!
//! A pilha de telas espelha o histórico da WebView: entrar numa nota ou pasta faz
//! `history.pushState`, e o botão Voltar do Android (ou o botão ← da tela) dispara `popstate`,
//! que tira a tela do topo. Assim o Voltar do Android volta da nota para a lista, e só fecha o
//! app na tela inicial.
//!
//! ENCAIXES das próximas fatias (cada uma acrescenta uma variante aqui e um braço em
//! `ui::tela`):
//! - R7b-2: `Leitura { caminho }`, a nota renderizada em Markdown;
//! - R7b-3: `Sincronizacao`, a tela de configurar e acompanhar o sync.

use dioxus::prelude::*;

use crate::ui::ponte;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rota {
    /// Pastas e notas de uma pasta do cofre (`""` = raiz).
    Lista { pasta: String },
    /// O editor de uma nota (caminho relativo, com `.md`).
    Nota { caminho: String },
}

impl Rota {
    pub fn raiz() -> Self {
        Rota::Lista {
            pasta: String::new(),
        }
    }
}

/// A pilha de telas, sem Dioxus (testável). A de baixo é sempre a lista da raiz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pilha {
    itens: Vec<Rota>,
}

impl Pilha {
    /// Começa em `inicial`, com a raiz embaixo se for outra tela.
    pub fn nova(inicial: Rota) -> Self {
        let mut itens = vec![Rota::raiz()];
        if inicial != Rota::raiz() {
            itens.push(inicial);
        }
        Self { itens }
    }

    pub fn topo(&self) -> &Rota {
        self.itens.last().expect("a pilha nunca fica vazia")
    }

    pub fn empurrar(&mut self, rota: Rota) {
        self.itens.push(rota);
    }

    /// Troca a tela do topo (nota renomeada).
    pub fn substituir_topo(&mut self, rota: Rota) {
        *self.itens.last_mut().expect("a pilha nunca fica vazia") = rota;
    }

    /// Tira o topo; a raiz nunca sai.
    pub fn desempilhar(&mut self) {
        if self.itens.len() > 1 {
            self.itens.pop();
        }
    }
}

/// Estado de navegação compartilhado por todas as telas (vem do contexto).
#[derive(Clone, Copy)]
pub struct Navegacao {
    pilha: Signal<Pilha>,
    /// Sobe a cada mudança no disco feita pelo app, para a lista se refazer.
    versao: Signal<u32>,
    /// Texto da busca (fica guardado ao entrar numa nota e voltar).
    busca: Signal<String>,
}

impl Navegacao {
    pub fn nova(inicial: Rota) -> Self {
        Self {
            pilha: Signal::new(Pilha::nova(inicial)),
            versao: Signal::new(0),
            busca: Signal::new(String::new()),
        }
    }

    /// A tela de agora (quem chama passa a ser redesenhado quando ela mudar).
    pub fn rota(&self) -> Rota {
        self.pilha.read().topo().clone()
    }

    /// Vai para uma tela nova, empilhando-a no histórico da WebView.
    pub fn ir(mut self, destino: Rota) {
        ponte::empurrar_historico();
        self.pilha.write().empurrar(destino);
    }

    /// Volta uma tela (pelo histórico, para o Voltar do Android e o botão ← andarem juntos).
    pub fn voltar(self) {
        ponte::voltar_historico();
    }

    /// Troca a tela de agora sem mexer no histórico (nota renomeada).
    pub fn substituir(mut self, destino: Rota) {
        self.pilha.write().substituir_topo(destino);
    }

    /// O sistema voltou uma entrada do histórico (`popstate`): tira a tela do topo.
    pub fn voltou_pelo_sistema(mut self) {
        self.pilha.write().desempilhar();
    }

    /// Avisa que o disco mudou (a lista relê).
    pub fn disco_mudou(mut self) {
        let v = *self.versao.peek();
        self.versao.set(v.wrapping_add(1));
    }

    /// Número da versão do disco; ler isto dentro de um componente o assina.
    pub fn versao(&self) -> u32 {
        *self.versao.read()
    }

    pub fn busca(&self) -> String {
        self.busca.read().clone()
    }

    /// O texto da busca sem assinar o componente (para o valor inicial do campo).
    pub fn busca_guardada(&self) -> String {
        self.busca.peek().clone()
    }

    pub fn definir_busca(mut self, texto: String) {
        self.busca.set(texto);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn nota(c: &str) -> Rota {
        Rota::Nota { caminho: c.into() }
    }

    #[test]
    fn comeca_na_raiz() {
        let p = Pilha::nova(Rota::raiz());
        assert_eq!(p.topo(), &Rota::raiz());
    }

    #[test]
    fn entra_e_volta() {
        let mut p = Pilha::nova(Rota::raiz());
        p.empurrar(Rota::Lista {
            pasta: "Trabalho".into(),
        });
        p.empurrar(nota("Trabalho/ata.md"));
        assert_eq!(p.topo(), &nota("Trabalho/ata.md"));
        p.desempilhar();
        assert_eq!(
            p.topo(),
            &Rota::Lista {
                pasta: "Trabalho".into()
            }
        );
        p.desempilhar();
        assert_eq!(p.topo(), &Rota::raiz());
    }

    #[test]
    fn a_raiz_nunca_sai() {
        let mut p = Pilha::nova(Rota::raiz());
        p.desempilhar();
        p.desempilhar();
        assert_eq!(p.topo(), &Rota::raiz());
    }

    #[test]
    fn renomear_troca_so_o_topo() {
        let mut p = Pilha::nova(nota("a.md"));
        p.substituir_topo(nota("b.md"));
        assert_eq!(p.topo(), &nota("b.md"));
        p.desempilhar();
        assert_eq!(p.topo(), &Rota::raiz());
    }

    #[test]
    fn inicial_que_nao_e_a_raiz_fica_sobre_ela() {
        let mut p = Pilha::nova(nota("x.md"));
        assert_eq!(p.topo(), &nota("x.md"));
        p.desempilhar();
        assert_eq!(p.topo(), &Rota::raiz());
    }
}
