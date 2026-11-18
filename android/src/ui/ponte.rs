//! A ponte com a WebView (JavaScript): o que o Rust não enxerga sozinho.
//!
//! - `popstate`: o botão Voltar do Android (ou `history.back()`) andou no histórico;
//! - visibilidade: o app foi para segundo plano (hora de gravar a nota);
//! - tique: de 0,6 em 0,6 s, enquanto o app está na tela, para o salvamento automático
//!   conferir se o dono parou de digitar (o Rust não tem temporizador próprio aqui).

use dioxus::prelude::*;

/// Roda uma vez na vida do app; fica escutando e manda texto para o Rust.
pub const SCRIPT: &str = r#"
window.addEventListener('popstate', () => dioxus.send('voltar'));
document.addEventListener('visibilitychange', () => {
  if (document.hidden) dioxus.send('oculto');
});
window.addEventListener('pagehide', () => dioxus.send('oculto'));
setInterval(() => {
  if (!document.hidden) dioxus.send('tique');
}, 600);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evento {
    /// O histórico voltou uma entrada.
    Voltou,
    /// O app saiu da tela (segundo plano, tela bloqueada, outro app).
    Oculto,
    /// Passou um intervalo; confira o salvamento automático.
    Tique,
}

pub fn interpretar(mensagem: &str) -> Option<Evento> {
    match mensagem {
        "voltar" => Some(Evento::Voltou),
        "oculto" => Some(Evento::Oculto),
        "tique" => Some(Evento::Tique),
        _ => None,
    }
}

/// Acrescenta uma entrada ao histórico (mesma URL): é ela que o Voltar do Android consome.
pub fn empurrar_historico() {
    let _ = document::eval("history.pushState({}, '');");
}

/// Volta uma entrada do histórico (dispara `popstate`).
pub fn voltar_historico() {
    let _ = document::eval("history.back();");
}

/// Seleciona o texto do campo do diálogo aberto (para trocar o nome de uma vez).
pub fn selecionar_campo_do_dialogo() {
    let _ = document::eval(
        "const c = document.querySelector('.dialogo input'); if (c) { c.focus(); c.select(); }",
    );
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn mensagens_conhecidas() {
        assert_eq!(interpretar("voltar"), Some(Evento::Voltou));
        assert_eq!(interpretar("oculto"), Some(Evento::Oculto));
        assert_eq!(interpretar("tique"), Some(Evento::Tique));
        assert_eq!(interpretar("outra"), None);
    }
}
