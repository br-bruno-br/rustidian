//! Rustidian para Android: caderno de notas Markdown em Dioxus 0.7 (R7b-1), com leitura formatada
//! (R7b-2) e sincronização com o servidor próprio (R7b-3). O cofre é uma pasta de arquivos `.md`
//! comuns. Roteiros de teste: docs/bruno/R7B-TESTE.md, R7B2-TESTE.md e R7B3-TESTE.md.
//!
//! Organização:
//! - `logica/`: regras puras (listar, buscar, nomes, salvamento), testadas sem Dioxus;
//! - `servicos`, `rota`, `edicao`, `vista`: estado compartilhado (cofre e sincronização abertos,
//!   navegação, nota aberta, o que a sincronização mudou);
//! - `ui/`: as telas (lista, nota, leitura e sincronizar), os diálogos e a ponte com a WebView;
//! - `estilo.css`: o visual, claro e escuro.
#![forbid(unsafe_code)]

mod edicao;
mod logica;
mod rota;
mod servicos;
mod ui;
mod vista;

#[cfg(test)]
mod prints;
#[cfg(test)]
mod testes_sync;

fn main() {
    dioxus::launch(ui::App);
}
