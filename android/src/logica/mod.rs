//! Lógica pura do app (sem Dioxus): tudo aqui roda e é testado no Windows com `cargo test`.
//! A tela só chama estas funções; nada aqui sabe de componente, sinal ou WebView.

pub mod agenda;
pub mod ajustes;
pub mod busca;
pub mod cofre;
pub mod conexao;
pub mod leitura;
pub mod local;
pub mod markdown;
pub mod nomes;
pub mod ordem;
pub mod portao;
pub mod rascunho;
pub mod sincronia;
