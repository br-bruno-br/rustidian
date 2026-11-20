//! Tela Sincronizar (R7b-3): configurar o servidor (endereço, token e frase-senha) e ver como
//! está a sincronização: última vez, erro em linguagem simples e as cópias de conflito.
//!
//! A frase-senha não é guardada em lugar nenhum: o app a pede a cada vez que é aberto (a chave
//! do cofre vive só na memória). O motivo está no PLANO, "Decisão da frase".

use dioxus::prelude::*;

use crate::logica::ajustes::APARELHO_PADRAO;
use crate::logica::nomes::{pasta_de, titulo_da_nota, ultimo_componente};
use crate::logica::sincronia::{Entrada, Fase, Tom};
use crate::rota::{Navegacao, Rota};
use crate::servicos::Servicos;
use crate::vista::VistaSync;

use super::dialogo::DialogoConfirmar;

/// Quantos conflitos a tela lista (o resto vira "e mais N").
const MAX_CONFLITOS: usize = 50;

#[component]
pub fn Sincronizar() -> Element {
    let servicos = use_context::<Servicos>();
    let nav = use_context::<Navegacao>();
    let vista = use_context::<VistaSync>();
    // Redesenha quando o estado muda (o tique da ponte atualiza a versão).
    let _ = vista.versao();
    let mut desconectando = use_signal(|| false);

    let estado = servicos.sync.estado();
    let ajustes = servicos.sync.ajustes();

    // Os campos não são reescritos a cada tecla (cursor e teclado): `value` fica fixo.
    let inicial = use_hook({
        let a = ajustes.clone();
        move || {
            (
                a.servidor.clone(),
                if a.aparelho.trim().is_empty() {
                    APARELHO_PADRAO.to_string()
                } else {
                    a.aparelho.clone()
                },
            )
        }
    });
    let mut servidor = use_signal({
        let v = inicial.0.clone();
        move || v
    });
    let mut aparelho = use_signal({
        let v = inicial.1.clone();
        move || v
    });
    let mut token = use_signal(String::new);
    let mut frase = use_signal(String::new);

    let rotulo = estado.rotulo();
    let conectando = estado.fase == Fase::Conectando;
    let agora = chrono::Local::now().naive_local();
    let ultima = estado.ultima_em_texto(agora);
    let ligada = matches!(
        estado.fase,
        Fase::Sincronizando | Fase::Sincronizado | Fase::SemConexao | Fase::Falhou
    );
    let tom = tom_css(rotulo.tom);

    let conectar = {
        let sync = servicos.sync.clone();
        move |_| {
            sync.conectar_em_segundo_plano(Entrada {
                servidor: servidor.read().clone(),
                aparelho: aparelho.read().clone(),
                token: token.read().clone(),
                frase: frase.read().clone(),
            });
        }
    };
    let entrar = {
        let sync = servicos.sync.clone();
        move |_| sync.entrar_em_segundo_plano(frase.read().clone())
    };

    let dialogo = if *desconectando.read() {
        rsx! {
            DialogoConfirmar {
                titulo: "Desconectar a sincronização?",
                mensagem: "O token e o controle interno da sincronização são apagados deste celular. Suas notas, o servidor e a lixeira não mudam.",
                rotulo_ok: "Desconectar",
                on_cancelar: move |_| desconectando.set(false),
                on_ok: {
                    let sync = servicos.sync.clone();
                    move |_| {
                        desconectando.set(false);
                        let _ = sync.desconectar();
                    }
                },
            }
        }
    } else {
        rsx! {}
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
                div { class: "titulos",
                    h1 { "Sincronizar" }
                }
            }
            main { class: "corpo sinc",
                div { class: "cartao-estado {tom}", role: "status",
                    span { class: "estado-texto", "{rotulo.texto}" }
                    if let Some(u) = ultima.clone() {
                        span { class: "sub", "Última sincronização: {u}" }
                    } else if ligada {
                        span { class: "sub", "Ainda não terminou nenhuma sincronização." }
                    }
                }

                if let Some(msg) = estado.erro.clone() {
                    div { class: "aviso erro-sinc", role: "alert",
                        span { "{msg}" }
                    }
                }
                if let Some(motivo) = estado.motivo_rede.clone() {
                    p { class: "detalhe", "Detalhe técnico: {motivo}" }
                }
                if let Some(texto) = estado.texto_em_massa() {
                    div { class: "aviso", role: "alert",
                        span { "{texto}" }
                        button {
                            class: "botao",
                            onclick: {
                                let sync = servicos.sync.clone();
                                move |_| sync.permitir_exclusao_em_massa_uma_vez()
                            },
                            "Permitir uma vez"
                        }
                    }
                }

                if !estado.conflitos.is_empty() {
                    h2 { class: "secao", "Conflitos ({estado.conflitos.len()})" }
                    p { class: "detalhe",
                        "A nota mudou aqui e em outro aparelho. As duas versões foram guardadas: a nota original e a cópia “(conflito …)”. Abra, junte o que quiser e mande a cópia que sobrar para a lixeira."
                    }
                    ul { class: "lista",
                        for c in estado.conflitos.iter().take(MAX_CONFLITOS) {
                            LinhaConflito { key: "{c}", caminho: c.clone() }
                        }
                    }
                    if estado.conflitos.len() > MAX_CONFLITOS {
                        p { class: "detalhe", "e mais {estado.conflitos.len() - MAX_CONFLITOS}" }
                    }
                }

                if !estado.recusados.is_empty() {
                    h2 { class: "secao", "Deixados de fora" }
                    ul { class: "recusados",
                        for r in estado.recusados.iter().take(MAX_CONFLITOS) {
                            li { "{r}" }
                        }
                    }
                }

                if ligada {
                    div { class: "bloco",
                        p { class: "detalhe",
                            "Servidor: {ajustes.servidor} · aparelho: {ajustes.aparelho}"
                        }
                        button {
                            class: "botao principal",
                            disabled: estado.fase == Fase::Sincronizando,
                            onclick: {
                                let sync = servicos.sync.clone();
                                move |_| sync.agora()
                            },
                            "Sincronizar agora"
                        }
                        button {
                            class: "botao",
                            onclick: move |_| desconectando.set(true),
                            "Desconectar"
                        }
                    }
                } else if ajustes.conectado {
                    div { class: "bloco",
                        p { class: "detalhe",
                            "Conectado a {ajustes.servidor} como “{ajustes.aparelho}”. Para sincronizar, digite a frase-senha do cofre. Ela não fica guardada: o app pede de novo cada vez que é aberto."
                        }
                        label { class: "rotulo", "Frase-senha" }
                        input {
                            class: "campo",
                            r#type: "password",
                            autocomplete: "off",
                            autocapitalize: "off",
                            spellcheck: false,
                            enterkeyhint: "go",
                            oninput: move |e| frase.set(e.value()),
                        }
                        button {
                            class: "botao principal",
                            disabled: conectando,
                            onclick: entrar,
                            "Entrar"
                        }
                        button {
                            class: "botao",
                            onclick: move |_| desconectando.set(true),
                            "Desconectar"
                        }
                    }
                } else {
                    div { class: "bloco",
                        p { class: "aviso-rede",
                            "Só funciona pela Tailscale ou na rede de casa: o endereço é http:// (sem HTTPS). As notas vão cifradas de ponta a ponta; o servidor só guarda o que ele não consegue ler."
                        }
                        label { class: "rotulo", "Endereço do servidor" }
                        input {
                            class: "campo",
                            r#type: "text",
                            inputmode: "url",
                            placeholder: "http://100.x.y.z:8443",
                            value: "{inicial.0}",
                            autocomplete: "off",
                            autocapitalize: "off",
                            spellcheck: false,
                            oninput: move |e| servidor.set(e.value()),
                        }
                        label { class: "rotulo", "Nome deste aparelho" }
                        input {
                            class: "campo",
                            r#type: "text",
                            value: "{inicial.1}",
                            autocomplete: "off",
                            oninput: move |e| aparelho.set(e.value()),
                        }
                        label { class: "rotulo", "Token do aparelho" }
                        input {
                            class: "campo",
                            r#type: "password",
                            placeholder: "nst_…",
                            autocomplete: "off",
                            autocapitalize: "off",
                            spellcheck: false,
                            oninput: move |e| token.set(e.value()),
                        }
                        label { class: "rotulo", "Frase-senha do cofre" }
                        input {
                            class: "campo",
                            r#type: "password",
                            autocomplete: "off",
                            autocapitalize: "off",
                            spellcheck: false,
                            enterkeyhint: "go",
                            oninput: move |e| frase.set(e.value()),
                        }
                        p { class: "detalhe",
                            "A frase não fica guardada em lugar nenhum: o app a pede de novo cada vez que é aberto. Se este for o primeiro aparelho, ela cria o cofre (mínimo de 8 caracteres)."
                        }
                        button {
                            class: "botao principal",
                            disabled: conectando,
                            onclick: conectar,
                            if conectando { "Conectando…" } else { "Conectar" }
                        }
                    }
                }
            }
            {dialogo}
        }
    }
}

fn tom_css(tom: Tom) -> &'static str {
    match tom {
        Tom::Neutro => "tom-neutro",
        Tom::Bom => "tom-bom",
        Tom::Ocupado => "tom-ocupado",
        Tom::Atencao => "tom-atencao",
        Tom::Ruim => "tom-ruim",
    }
}

/// Uma cópia de conflito: toque para abrir no editor.
#[component]
fn LinhaConflito(caminho: String) -> Element {
    let nav = use_context::<Navegacao>();
    let nome = titulo_da_nota(ultimo_componente(&caminho)).to_string();
    let pasta = pasta_de(&caminho).to_string();
    rsx! {
        li { class: "linha",
            button {
                class: "linha-corpo",
                onclick: {
                    let caminho = caminho.clone();
                    move |_| nav.ir(Rota::Nota { caminho: caminho.clone() })
                },
                span { class: "tipo", aria_hidden: "true", "⚠" }
                span { class: "textos",
                    span { class: "nome", "{nome}" }
                    if !pasta.is_empty() {
                        span { class: "sub", "{pasta}" }
                    }
                }
            }
        }
    }
}
