//! Tela Nota: o editor de uma nota. Título = nome do arquivo (tocar para renomear),
//! salvamento automático e indicador "Salvo / Salvando…".
//!
//! ENCAIXE R7b-2: a visualização em Markdown entra aqui. O corpo da tela é a função
//! `editor`; a fatia acrescenta um botão no topo e troca o corpo por uma `Leitura` (rota nova
//! em `rota.rs`) sem mexer no salvamento, que é do `Edicao`.

use dioxus::prelude::*;

use crate::edicao::Edicao;
use crate::logica::cofre::explicar;
use crate::logica::nomes::{pasta_de, titulo_da_nota, ultimo_componente};
use crate::logica::rascunho::Estado;
use crate::rota::{Navegacao, Rota};
use crate::servicos::Servicos;

use super::dialogo::{DialogoConfirmar, DialogoTexto};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pedido {
    Renomear,
    Apagar,
}

#[component]
pub fn Nota(caminho: String) -> Element {
    let servicos = use_context::<Servicos>();
    let nav = use_context::<Navegacao>();
    let edicao = use_context::<Edicao>();
    let mut pedido = use_signal(|| None::<Pedido>);
    let mut aviso = use_signal(|| None::<String>);

    // Lê a nota uma vez, ao abrir. O texto digitado vive no `Edicao`; o <textarea> não é
    // reescrito a cada tecla.
    let aberta = use_hook({
        let cofre = servicos.cofre.clone();
        let caminho = caminho.clone();
        move || match cofre.ler_nota(&caminho) {
            Ok(texto) => {
                edicao.abrir(&caminho, texto.clone());
                Ok(texto)
            }
            Err(e) => Err(explicar(&e)),
        }
    });

    // Ao sair da tela (voltar, apagar, renomear): grava o que faltar e solta a nota.
    use_drop({
        let cofre = servicos.cofre.clone();
        let caminho = caminho.clone();
        move || {
            edicao.salvar(&cofre);
            edicao.fechar_se(&caminho);
        }
    });

    let titulo = titulo_da_nota(ultimo_componente(&caminho)).to_string();
    let pasta = pasta_de(&caminho).to_string();

    let dialogo = match *pedido.read() {
        None => rsx! {},
        Some(Pedido::Renomear) => rsx! {
            DialogoTexto {
                titulo: "Renomear a nota",
                rotulo_ok: "Renomear",
                valor: titulo.clone(),
                on_cancelar: move |_| pedido.set(None),
                on_ok: {
                    let cofre = servicos.cofre.clone();
                    let caminho = caminho.clone();
                    move |novo: String| {
                        pedido.set(None);
                        edicao.salvar(&cofre);
                        match cofre.renomear_nota(&caminho, &novo) {
                            Ok(novo_caminho) => {
                                if novo_caminho != caminho {
                                    edicao.mudou_de_caminho(&novo_caminho);
                                    nav.substituir(Rota::Nota {
                                        caminho: novo_caminho,
                                    });
                                    nav.disco_mudou();
                                }
                            }
                            Err(e) => aviso.set(Some(explicar(&e))),
                        }
                    }
                },
            }
        },
        Some(Pedido::Apagar) => rsx! {
            DialogoConfirmar {
                titulo: format!("Mover “{titulo}” para a lixeira?"),
                mensagem: "Ela vai para a lixeira do cofre (.trash). Nada é apagado de vez.",
                rotulo_ok: "Mover para a lixeira",
                on_cancelar: move |_| pedido.set(None),
                on_ok: {
                    let cofre = servicos.cofre.clone();
                    let caminho = caminho.clone();
                    move |_| {
                        pedido.set(None);
                        edicao.salvar(&cofre);
                        if edicao.pendente() {
                            aviso.set(Some(
                                "Não consegui salvar o texto, então não mexi na nota.".into(),
                            ));
                            return;
                        }
                        match cofre.para_a_lixeira(&caminho) {
                            Ok(_) => {
                                edicao.fechar();
                                nav.disco_mudou();
                                nav.voltar();
                            }
                            Err(e) => aviso.set(Some(explicar(&e))),
                        }
                    }
                },
            }
        },
    };

    rsx! {
        div { class: "tela",
            header { class: "topo",
                button {
                    class: "icone",
                    aria_label: "Voltar",
                    onclick: move |_| nav.voltar(),
                    "←"
                }
                button {
                    class: "titulo-nota",
                    aria_label: "Renomear a nota",
                    onclick: move |_| pedido.set(Some(Pedido::Renomear)),
                    span { class: "nome", "{titulo}" }
                    if !pasta.is_empty() {
                        span { class: "sub", "{pasta}" }
                    }
                }
                button {
                    class: "icone apagar",
                    aria_label: "Mover para a lixeira",
                    onclick: move |_| pedido.set(Some(Pedido::Apagar)),
                    "🗑"
                }
            }
            if let Some(msg) = aviso.read().clone() {
                div { class: "aviso", role: "alert",
                    span { "{msg}" }
                    button { class: "botao", onclick: move |_| aviso.set(None), "OK" }
                }
            }
            {editor(aberta, servicos.cofre.clone(), edicao)}
            {dialogo}
        }
    }
}

/// O corpo da tela: o campo de texto e o indicador de salvamento.
fn editor(
    aberta: Result<String, String>,
    cofre: crate::logica::cofre::Cofre,
    edicao: Edicao,
) -> Element {
    match aberta {
        Err(msg) => rsx! { p { class: "vazio erro", "{msg}" } },
        Ok(texto) => {
            let vazia = texto.is_empty();
            rsx! {
                // O texto inicial entra uma vez como conteúdo do campo. Nada reescreve o valor
                // depois: o Gboard e o cursor ficam com a WebView, como numa página comum.
                textarea {
                    class: "editor",
                    placeholder: "Escreva aqui…",
                    spellcheck: true,
                    autocomplete: "on",
                    autocapitalize: "sentences",
                    autofocus: vazia,
                    oninput: move |e| edicao.digitou(e.value()),
                    onblur: move |_| edicao.salvar(&cofre),
                    "{texto}"
                }
                Indicador {}
            }
        }
    }
}

/// "Salvo", "Salvando…" ou o erro. Fica separado para só ele se redesenhar a cada tecla.
#[component]
fn Indicador() -> Element {
    let edicao = use_context::<Edicao>();
    let (classe, texto) = match edicao.estado() {
        Estado::Salvo => ("estado salvo", "Salvo".to_string()),
        Estado::Salvando => ("estado salvando", "Salvando…".to_string()),
        Estado::Erro(e) => ("estado erro", format!("Não salvou: {e}")),
    };
    rsx! {
        div { class: "{classe}", role: "status", "{texto}" }
    }
}
