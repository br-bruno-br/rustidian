//! Rustidian para Android (R7b-1): caderno de notas Markdown em Dioxus 0.7, sem sincronização
//! ainda. O cofre é uma pasta de arquivos `.md` comuns. Roteiro de teste: docs/bruno/R7B-TESTE.md.
//!
//! Organização:
//! - `logica/`: regras puras (listar, buscar, nomes, salvamento), testadas sem Dioxus;
//! - `servicos`, `rota`, `edicao`: estado compartilhado (cofre aberto, navegação, nota aberta);
//! - `ui/`: as telas (lista e nota), os diálogos e a ponte com a WebView;
//! - `estilo.css`: o visual, claro e escuro.
#![forbid(unsafe_code)]

mod edicao;
mod logica;
mod rota;
mod servicos;
mod ui;


fn main() {
    dioxus::launch(ui::App);
}
