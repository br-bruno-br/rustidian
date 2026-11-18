//! Tela Lista: pastas e notas de uma pasta do cofre, busca, nota nova e pasta nova.

use dioxus::prelude::*;

use crate::logica::busca::{MAX_ACHADOS, buscar};
use crate::logica::cofre::{Item, Tipo, explicar};
use crate::logica::nomes::{PASTA_NOVA, pasta_de, ultimo_componente};
use crate::rota::{Navegacao, Rota};
use crate::servicos::Servicos;

use super::dialogo::{DialogoConfirmar, DialogoTexto};

/// O que o dono pediu e está esperando resposta num diálogo.
#[derive(Debug, Clone, PartialEq)]
enum Pedido {
    NovaPasta,
    Apagar(Item),
}

#[component]
pub fn Lista(pasta: String) -> Element {
    let servicos = use_context::<Servicos>();
    let nav = use_context::<Navegacao>();
    let mut pedido = use_signal(|| None::<Pedido>);
    let mut aviso = use_signal(|| None::<String>);

    // As duas listas só são refeitas quando muda a pasta, a busca ou o disco (não a cada
    // diálogo aberto).
    let itens = use_memo({
        let cofre = servicos.cofre.clone();
        let pasta = pasta.clone();
        move || {
            let _ = nav.versao();
            cofre.listar(&pasta).map_err(|e| explicar(&e))
        }
    });
    let achados = use_memo({
        let cofre = servicos.cofre.clone();
        move || {
            let _ = nav.versao();
            let consulta = nav.busca();
            if consulta.trim().is_empty() {
                None
            } else {
                Some(buscar(&cofre, &consulta))
            }
        }
    });
    // O campo de busca não é reescrito a cada tecla (cursor e teclado); só nasce com o texto
    // que já estava guardado.
    let busca_inicial = use_hook(move || nav.busca_guardada());

    let abrir = move |item: Item| match item.tipo {
        Tipo::Pasta => {
            nav.definir_busca(String::new());
            nav.ir(Rota::Lista {
                pasta: item.caminho,
            });
        }
        Tipo::Nota => nav.ir(Rota::Nota {
            caminho: item.caminho,
        }),
    };
    let pedir_apagar = move |item: Item| pedido.set(Some(Pedido::Apagar(item)));

    let nova_nota = {
        let cofre = servicos.cofre.clone();
        let pasta = pasta.clone();
        move |_| match cofre.criar_nota(&pasta) {
            Ok(caminho) => {
                nav.disco_mudou();
                nav.ir(Rota::Nota { caminho });
            }
            Err(e) => aviso.set(Some(explicar(&e))),
        }
    };

    let titulo = if pasta.is_empty() {
        "Rustidian".to_string()
    } else {
        ultimo_componente(&pasta).to_string()
    };
    let caminho_da_pasta = pasta_de(&pasta).to_string();
    let na_raiz = pasta.is_empty();

    let corpo = match &*achados.read() {
        Some(r) => {
            if r.achados.is_empty() {
                rsx! { p { class: "vazio", "Nada encontrado." } }
            } else {
                rsx! {
                    ul { class: "lista",
                        for a in r.achados.iter() {
                            Linha {
                                key: "{a.item.caminho}",
                                item: a.item.clone(),
                                trecho: a.trecho.clone(),
                                mostrar_pasta: true,
                                on_abrir: abrir,
                                on_apagar: pedir_apagar,
                            }
                        }
                    }
                    if r.truncado {
                        p { class: "vazio", "Mostrando só os primeiros {MAX_ACHADOS}. Escreva mais para afinar." }
                    }
                }
            }
        }
        None => match &*itens.read() {
            Err(msg) => rsx! { p { class: "vazio erro", "{msg}" } },
            Ok(lista) if lista.is_empty() => rsx! {
                p { class: "vazio",
                    if na_raiz {
                        "Seu cofre está vazio. Toque em Nova nota para começar, ou copie suas notas .md para a pasta do cofre."
                    } else {
                        "Esta pasta está vazia."
                    }
                }
            },
            Ok(lista) => rsx! {
                ul { class: "lista",
                    for item in lista.iter() {
                        Linha {
                            key: "{item.caminho}",
                            item: item.clone(),
                            trecho: None,
                            mostrar_pasta: false,
                            on_abrir: abrir,
                            on_apagar: pedir_apagar,
                        }
                    }
                }
            },
        },
    };

    let dialogo = match pedido.read().clone() {
        None => rsx! {},
        Some(Pedido::NovaPasta) => rsx! {
            DialogoTexto {
                titulo: "Nova pasta",
                rotulo_ok: "Criar",
                valor: PASTA_NOVA,
                on_cancelar: move |_| pedido.set(None),
                on_ok: {
                    let cofre = servicos.cofre.clone();
                    let pasta = pasta.clone();
                    move |nome: String| {
                        pedido.set(None);
                        match cofre.criar_pasta(&pasta, &nome) {
                            Ok(_) => nav.disco_mudou(),
                            Err(e) => aviso.set(Some(explicar(&e))),
                        }
                    }
                },
            }
        },
        Some(Pedido::Apagar(item)) => {
            let (titulo, mensagem) = match item.tipo {
                Tipo::Pasta => (
                    format!("Mover a pasta “{}” para a lixeira?", item.nome),
                    "A pasta e tudo que está dentro dela vão para a lixeira do cofre (.trash). Nada é apagado de vez.",
                ),
                Tipo::Nota => (
                    format!("Mover a nota “{}” para a lixeira?", item.nome),
                    "Ela vai para a lixeira do cofre (.trash). Nada é apagado de vez.",
                ),
            };
            rsx! {
                DialogoConfirmar {
                    titulo,
                    mensagem,
                    rotulo_ok: "Mover para a lixeira",
                    on_cancelar: move |_| pedido.set(None),
                    on_ok: {
                        let cofre = servicos.cofre.clone();
                        move |_| {
                            pedido.set(None);
                            match cofre.para_a_lixeira(&item.caminho) {
                                Ok(_) => nav.disco_mudou(),
                                Err(e) => aviso.set(Some(explicar(&e))),
                            }
                        }
                    },
                }
            }
        }
    };

    rsx! {
        div { class: "tela",
            header { class: "topo",
                if !na_raiz {
                    button {
                        class: "icone",
                        aria_label: "Voltar",
                        onclick: move |_| nav.voltar(),
                        "←"
                    }
                }
                div { class: "titulos",
                    h1 { "{titulo}" }
                    if !caminho_da_pasta.is_empty() {
                        span { class: "sub", "{caminho_da_pasta}" }
                    }
                }
            }
            div { class: "busca",
                input {
                    class: "campo",
                    r#type: "search",
                    placeholder: "Buscar por nome ou texto",
                    value: "{busca_inicial}",
                    autocomplete: "off",
                    enterkeyhint: "search",
                    oninput: move |e| nav.definir_busca(e.value()),
                }
            }
            if let Some(msg) = aviso.read().clone() {
                div { class: "aviso", role: "alert",
                    span { "{msg}" }
                    button { class: "botao", onclick: move |_| aviso.set(None), "OK" }
                }
            }
            if na_raiz {
                if let Some(msg) = servicos.aviso.clone() {
                    div { class: "aviso", span { "{msg}" } }
                }
            }
            main { class: "corpo", {corpo} }
            if na_raiz {
                p { class: "rodape-cofre",
                    "Cofre: {servicos.cofre.raiz().display()}"
                }
            }
            footer { class: "acoes",
                button { class: "botao principal", onclick: nova_nota, "Nova nota" }
                button {
                    class: "botao",
                    onclick: move |_| pedido.set(Some(Pedido::NovaPasta)),
                    "Nova pasta"
                }
            }
            {dialogo}
        }
    }
}

/// Uma linha da lista (pasta ou nota) com o botão da lixeira.
#[component]
fn Linha(
    item: Item,
    trecho: Option<String>,
    mostrar_pasta: bool,
    on_abrir: EventHandler<Item>,
    on_apagar: EventHandler<Item>,
) -> Element {
    let icone = match item.tipo {
        Tipo::Pasta => "📁",
        Tipo::Nota => "📝",
    };
    let pai = pasta_de(&item.caminho).to_string();
    let item_abrir = item.clone();
    let item_apagar = item.clone();
    rsx! {
        li { class: "linha",
            button {
                class: "linha-corpo",
                onclick: move |_| on_abrir.call(item_abrir.clone()),
                span { class: "tipo", aria_hidden: "true", "{icone}" }
                span { class: "textos",
                    span { class: "nome", "{item.nome}" }
                    if mostrar_pasta && !pai.is_empty() {
                        span { class: "sub", "{pai}" }
                    }
                    if let Some(t) = &trecho {
                        span { class: "trecho", "{t}" }
                    }
                }
            }
            button {
                class: "icone apagar",
                aria_label: "Mover para a lixeira",
                onclick: move |_| on_apagar.call(item_apagar.clone()),
                "🗑"
            }
        }
    }
}
