pub mod assets;
pub mod atualizacao;
pub mod autosave;
pub mod board_note;
pub mod bookmarks;
pub mod callouts;
pub mod canvas;
pub mod canvas_index;
pub mod commands;
pub mod editor;
pub mod folding;
pub mod graph;
pub mod graph_layout;
pub mod highlight;
pub mod html_import;
pub mod html_layout;
pub mod i18n;
pub mod index_photo;
pub mod inline_style;
pub mod install_linux;
pub mod math;
pub mod metadata;
pub mod native_picker;
pub mod navigation;
pub mod note_aids;
pub mod operations;
pub mod panes;
pub mod paths;
pub mod perf;
pub mod query;
pub mod recovery;
pub mod recovery_capture;
pub mod search;
pub mod spell;
pub mod structure;
pub mod sync;
pub mod tab;
pub mod tables;
pub mod unlinked;
#[cfg(all(feature = "agente", not(target_os = "linux")))]
compile_error!(
    "A feature `agente` só funciona no Linux por enquanto: o terminal usa PTY e sinais POSIX (a tabela de R2 do plano do fork)."
);

/// Aba do agente: desligada por padrão (revisão de segurança de 15/11/2026).
#[cfg(feature = "agente")]
pub mod terminal;
pub mod vault;
pub mod vaults;
pub mod watcher;
pub mod workspaces;

pub const APP_NAME: &str = "Rustidian";
