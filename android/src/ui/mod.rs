//! A interface (Dioxus). O roteamento mora aqui, em `tela`; as telas são módulos separados.

use dioxus::prelude::*;

use crate::edicao::Edicao;
use crate::rota::{Navegacao, Rota};
use crate::servicos::Servicos;

pub mod dialogo;
pub mod leitura;
pub mod lista;
pub mod nota;
pub mod ponte;

/// O CSS do app inteiro (claro e escuro pelo tema do sistema).
pub const CSS: &str = include_str!("../estilo.css");

/// O componente raiz do app: abre o cofre ou mostra por que não conseguiu.
#[component]
pub fn App() -> Element {
    let aberto = use_hook(Servicos::abrir);
    match aberto {
        Ok(servicos) => rsx! { Raiz { servicos } },
        Err(mensagem) => rsx! {
            style { dangerous_inner_html: CSS }
            div { class: "tela",
                header { class: "topo",
                    div { class: "titulos", h1 { "Rustidian" } }
                }
                p { class: "vazio erro", "{mensagem}" }
            }
        },
    }
}

/// Tudo que vale com o cofre aberto: contextos, ponte com a WebView e a tela de agora.
/// `inicial` serve aos testes e aos prints (abrir direto numa tela).
#[component]
pub fn Raiz(servicos: Servicos, inicial: Option<Rota>) -> Element {
    let cofre = servicos.cofre.clone();
    use_context_provider(|| servicos.clone());
    let nav = use_context_provider(|| Navegacao::nova(inicial.clone().unwrap_or_else(Rota::raiz)));
    let edicao = use_context_provider(Edicao::nova);

    // Escuta a WebView: Voltar do Android, app indo para segundo plano e o tique do
    // salvamento automático.
    use_future(move || {
        let cofre = cofre.clone();
        async move {
            let mut ponte = document::eval(ponte::SCRIPT);
            while let Ok(mensagem) = ponte.recv::<String>().await {
                match ponte::interpretar(&mensagem) {
                    Some(ponte::Evento::Voltou) => {
                        // Grava antes de a tela da nota sair do ar.
                        edicao.salvar(&cofre);
                        nav.voltou_pelo_sistema();
                    }
                    Some(ponte::Evento::Oculto) => edicao.salvar(&cofre),
                    Some(ponte::Evento::Tique) => edicao.salvar_se_vencido(&cofre),
                    None => {}
                }
            }
        }
    });

    rsx! {
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, interactive-widget=resizes-content",
        }
        style { dangerous_inner_html: CSS }
        {tela(nav.rota())}
    }
}

/// O único lugar que liga uma rota à sua tela. ENCAIXE R7b-3: um braço por tela nova.
fn tela(rota: Rota) -> Element {
    match rota {
        Rota::Lista { pasta } => rsx! { lista::Lista { key: "lista-{pasta}", pasta } },
        Rota::Nota { caminho } => rsx! { nota::Nota { key: "nota-{caminho}", caminho } },
        Rota::Leitura { caminho } => {
            rsx! { leitura::Leitura { key: "leitura-{caminho}", caminho } }
        }
    }
}
