//! Tela Leitura (R7b-2): a nota lida com formatação (títulos, listas, tabelas, tarefas, código).
//!
//! O texto continua sendo o arquivo: aqui nada é editado, só a caixa de tarefa grava (troca o
//! `[ ]` daquela linha e mais nada). Todo o HTML vem de `logica::markdown`, que nunca repassa
//! HTML cru da nota; os toques em link e em caixa chegam pela ponte (`ponte::SCRIPT_LEITURA`)
//! como um número que indexa as tabelas da página, nunca como endereço.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::logica::cofre::explicar;
use crate::logica::leitura::{self, ErroTarefa, MundoDoCofre};
use crate::logica::markdown::{self, Alvo};
use crate::logica::nomes::{pasta_de, titulo_da_nota, ultimo_componente};
use crate::rota::{Navegacao, Rota};
use crate::servicos::Servicos;

use super::ponte::{self, Toque};

#[component]
pub fn Leitura(caminho: String) -> Element {
    let servicos = use_context::<Servicos>();
    let nav = use_context::<Navegacao>();
    let mut aviso = use_signal(|| None::<String>);

    // O texto que a tela está mostrando (e que a caixa de tarefa confere com o disco).
    let mut aberta = use_signal({
        let cofre = servicos.cofre.clone();
        let caminho = caminho.clone();
        move || cofre.ler_nota(&caminho).map_err(|e| explicar(&e))
    });
    // O índice de nomes do cofre (para `[[nota]]` e imagens), montado uma vez ao abrir.
    let mundo = use_hook({
        let cofre = servicos.cofre.clone();
        move || Rc::new(MundoDoCofre::novo(&cofre))
    });
    let pagina = use_memo({
        let caminho = caminho.clone();
        let mundo = mundo.clone();
        move || {
            aberta
                .read()
                .as_ref()
                .ok()
                .map(|texto| markdown::renderizar(texto, &caminho, &*mundo))
        }
    });

    // Escuta os toques em link e em caixa de tarefa.
    use_future({
        let cofre = servicos.cofre.clone();
        let caminho = caminho.clone();
        move || {
            let cofre = cofre.clone();
            let caminho = caminho.clone();
            async move {
                let mut canal = document::eval(ponte::SCRIPT_LEITURA);
                while let Ok(mensagem) = canal.recv::<String>().await {
                    match ponte::interpretar_toque(&mensagem) {
                        Some(Toque::Alvo(i)) => {
                            let alvo = pagina.peek().as_ref().and_then(|p| p.alvos.get(i).cloned());
                            match alvo {
                                Some(Alvo::Nota(destino)) => {
                                    if destino != caminho {
                                        nav.ir(Rota::Leitura { caminho: destino });
                                    }
                                }
                                Some(Alvo::Web(url))
                                    if leitura::abrir_no_navegador(&url).is_err() =>
                                {
                                    // Sem navegador (ou o `am` falhou): o endereço fica à vista.
                                    aviso.set(Some(format!(
                                        "Não consegui abrir o navegador. Endereço: {url}"
                                    )));
                                }
                                Some(Alvo::Web(_)) | None => {}
                            }
                        }
                        Some(Toque::Tarefa(i)) => {
                            let byte = pagina
                                .peek()
                                .as_ref()
                                .and_then(|p| p.tarefas.get(i).map(|t| t.byte));
                            let base = aberta.peek().as_ref().ok().cloned();
                            let (Some(byte), Some(base)) = (byte, base) else {
                                continue;
                            };
                            match leitura::alternar_tarefa(&cofre, &caminho, &base, byte) {
                                Ok(novo) => aberta.set(Ok(novo)),
                                Err(ErroTarefa::Mudou) => {
                                    // Mudou por fora (sync, outro app): mostra a versão nova e
                                    // deixa o dono tocar de novo, já olhando o que mudou.
                                    aberta.set(cofre.ler_nota(&caminho).map_err(|e| explicar(&e)));
                                    aviso.set(Some(
                                        "A nota mudou fora do app. Mostrei a versão nova; toque de novo se ainda quiser marcar.".into(),
                                    ));
                                }
                                Err(ErroTarefa::Sumiu) => {
                                    aberta.set(cofre.ler_nota(&caminho).map_err(|e| explicar(&e)));
                                    aviso.set(Some(
                                        "Essa tarefa não está mais na nota. Mostrei a versão atual."
                                            .into(),
                                    ));
                                }
                                Err(ErroTarefa::Disco(e)) => aviso.set(Some(format!(
                                    "Não consegui gravar a tarefa: {}",
                                    explicar(&e)
                                ))),
                            }
                        }
                        None => {}
                    }
                }
            }
        }
    });

    // Ao sair da tela o ouvinte de toques da WebView é desligado.
    use_drop(|| {
        let _ = document::eval(ponte::DESLIGAR_LEITURA);
    });

    let titulo = titulo_da_nota(ultimo_componente(&caminho)).to_string();
    let pasta = pasta_de(&caminho).to_string();

    rsx! {
        div { class: "tela",
            header { class: "topo",
                button {
                    class: "icone",
                    aria_label: "Voltar",
                    onclick: move |_| nav.voltar(),
                    "←"
                }
                div { class: "titulo-nota leitura-titulo",
                    span { class: "nome", "{titulo}" }
                    if !pasta.is_empty() {
                        span { class: "sub", "{pasta}" }
                    }
                }
                button {
                    class: "alterna",
                    aria_label: "Editar a nota",
                    onclick: {
                        let caminho = caminho.clone();
                        move |_| nav.substituir(Rota::Nota { caminho: caminho.clone() })
                    },
                    "Editar"
                }
            }
            if let Some(msg) = aviso.read().clone() {
                div { class: "aviso", role: "alert",
                    span { "{msg}" }
                    button { class: "botao", onclick: move |_| aviso.set(None), "OK" }
                }
            }
            {corpo(&aberta.read(), pagina.read().as_ref())}
        }
    }
}

/// O texto formatado, ou o motivo de não ter aberto, ou o aviso de nota vazia.
fn corpo(aberta: &Result<String, String>, pagina: Option<&markdown::Pagina>) -> Element {
    match (aberta, pagina) {
        (Err(msg), _) => rsx! { p { class: "vazio erro", "{msg}" } },
        (Ok(texto), _) if texto.trim().is_empty() => rsx! {
            p { class: "vazio", "Esta nota está vazia. Toque em Editar para escrever." }
        },
        (Ok(_), Some(p)) => rsx! {
            div { class: "corpo",
                article { class: "leitura", dangerous_inner_html: "{p.html}" }
            }
        },
        (Ok(_), None) => rsx! {},
    }
}
