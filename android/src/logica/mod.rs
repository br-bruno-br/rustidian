//! Lógica pura do app (sem Dioxus): tudo aqui roda e é testado no Windows com `cargo test`.
//! A tela só chama estas funções; nada aqui sabe de componente, sinal ou WebView.

pub mod busca;
pub mod cofre;
pub mod local;
pub mod nomes;
pub mod ordem;
pub mod rascunho;
