//! Caixas de diálogo sobre a tela: pedir um nome e confirmar uma ação.
//! Tocar fora da caixa cancela. Os botões têm pelo menos 48 px de altura.

use dioxus::prelude::*;

use super::ponte;

/// Pede um texto (nome de pasta ou título de nota).
#[component]
pub fn DialogoTexto(
    titulo: String,
    rotulo_ok: String,
    valor: String,
    on_ok: EventHandler<String>,
    on_cancelar: EventHandler<()>,
) -> Element {
    let mut digitado = use_signal({
        let valor = valor.clone();
        move || valor
    });
    // Seleciona o nome atual para o dono poder trocar de uma vez.
    use_effect(ponte::selecionar_campo_do_dialogo);
    rsx! {
        div { class: "fundo", onclick: move |_| on_cancelar.call(()),
            div {
                class: "dialogo",
                role: "dialog",
                onclick: move |e| e.stop_propagation(),
                h2 { "{titulo}" }
                // `value` fica fixo (o que veio de fora): o campo não é reescrito a cada tecla,
                // senão o cursor pula e a composição do teclado quebra.
                input {
                    class: "campo",
                    r#type: "text",
                    value: "{valor}",
                    autofocus: true,
                    autocomplete: "off",
                    enterkeyhint: "done",
                    oninput: move |e| digitado.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            on_ok.call(digitado.read().clone());
                        }
                    },
                }
                div { class: "botoes",
                    button { class: "botao", onclick: move |_| on_cancelar.call(()), "Cancelar" }
                    button {
                        class: "botao principal",
                        onclick: move |_| on_ok.call(digitado.read().clone()),
                        "{rotulo_ok}"
                    }
                }
            }
        }
    }
}

/// Pede confirmação de uma ação (apagar vai para a lixeira, nunca some de vez).
#[component]
pub fn DialogoConfirmar(
    titulo: String,
    mensagem: String,
    rotulo_ok: String,
    on_ok: EventHandler<()>,
    on_cancelar: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "fundo", onclick: move |_| on_cancelar.call(()),
            div {
                class: "dialogo",
                role: "alertdialog",
                onclick: move |e| e.stop_propagation(),
                h2 { "{titulo}" }
                p { class: "mensagem", "{mensagem}" }
                div { class: "botoes",
                    button { class: "botao", onclick: move |_| on_cancelar.call(()), "Cancelar" }
                    button { class: "botao perigo", onclick: move |_| on_ok.call(()), "{rotulo_ok}" }
                }
            }
        }
    }
}
