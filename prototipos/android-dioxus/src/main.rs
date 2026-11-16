//! Protótipo R7a (Dioxus): o mesmo teste do protótipo egui, mas com um <textarea> numa WebView.
//! O teclado fala com a WebView do Android (a mesma do Chrome), então o Gboard deve se
//! comportar como em qualquer página. Roteiro de teste: docs/bruno/R7-TESTE.md.

use dioxus::prelude::*;
use std::path::PathBuf;

const PACOTE: &str = "dev.rustidian.protodioxus";

/// Pasta interna do app. No Android o caminho é fixo a partir do nome do pacote
/// (/data/data/<pacote>/files); no PC usamos a pasta temporária.
fn pasta() -> PathBuf {
    if cfg!(target_os = "android") {
        let p = PathBuf::from(format!("/data/data/{PACOTE}/files"));
        let _ = std::fs::create_dir_all(&p);
        p
    } else {
        std::env::temp_dir()
    }
}

fn main() {
    dioxus::launch(App);
}

const CSS: &str = r#"
html, body { margin: 0; height: 100%; }
body { font-family: sans-serif; background: #fff; color: #111; }
.tela { box-sizing: border-box; height: 100dvh; display: flex; flex-direction: column;
        padding: 8px 12px; gap: 6px; }
.topo { display: flex; align-items: center; gap: 12px; }
.topo h1 { font-size: 20px; margin: 0; }
.topo button { font-size: 18px; padding: 8px 18px; }
.status { font-size: 13px; margin: 0; word-break: break-all; color: #333; }
textarea { flex: 1; min-height: 0; width: 100%; box-sizing: border-box; font-size: 18px;
           line-height: 1.4; padding: 8px; resize: none; }
.rodape { font-size: 12px; color: #555; }
@media (prefers-color-scheme: dark) {
  body { background: #111; color: #eee; } .status { color: #ccc; } .rodape { color: #aaa; }
  textarea { background: #222; color: #eee; }
}
"#;

#[component]
fn App() -> Element {
    let arquivo = pasta().join("nota.md");
    // Texto inicial lido uma vez. O <textarea> NÃO é controlado (sem `value`): reescrever o
    // valor a cada tecla pelo Rust faria o cursor pular e quebraria a composição do teclado.
    let (inicial, status_inicial) = use_hook({
        let arquivo = arquivo.clone();
        move || match std::fs::read_to_string(&arquivo) {
            Ok(t) => {
                let n = t.len();
                (t, format!("Lido de {} ({n} bytes)", arquivo.display()))
            }
            Err(_) => (String::new(), format!("Ainda sem arquivo em {}", arquivo.display())),
        }
    });
    let mut texto = use_signal(|| inicial.clone());
    let mut status = use_signal(|| status_inicial);
    let mut eventos = use_signal(|| 0u32);

    rsx! {
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, interactive-widget=resizes-content",
        }
        style { "{CSS}" }
        div { class: "tela",
            div { class: "topo",
                h1 { "Proto Dioxus" }
                button {
                    onclick: move |_| {
                        let t = texto.read().clone();
                        let r = std::fs::write(&arquivo, t.as_bytes());
                        status.set(match r {
                            Ok(()) => format!(
                                "Salvo: {} bytes, {} caracteres em {}",
                                t.len(), t.chars().count(), arquivo.display()
                            ),
                            Err(e) => format!("ERRO ao salvar: {e}"),
                        });
                    },
                    "Salvar"
                }
            }
            p { class: "status", "{status}" }
            textarea {
                placeholder: "Toque aqui e digite",
                spellcheck: true,
                autocomplete: "on",
                oninput: move |e| {
                    texto.set(e.value());
                    eventos += 1;
                },
                "{inicial}"
            }
            div { class: "rodape",
                "{texto.read().chars().count()} caracteres | eventos de digitação: {eventos}"
            }
        }
    }
}
