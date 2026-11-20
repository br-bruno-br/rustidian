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

/// Só enquanto a tela de Leitura está aberta: toques em link (`data-a`) e em caixa de tarefa
/// (`data-t`) do HTML gerado por `logica::markdown`. O toque NUNCA segue o comportamento
/// padrão do navegador (`preventDefault` na captura) e qualquer `<a href>` que escapasse
/// também é barrado: a WebView não navega para fora. Ao sair da tela, `DESLIGAR_LEITURA`.
pub const SCRIPT_LEITURA: &str = r#"
if (window.__leituraOff) window.__leituraOff();
const aoTocar = (e) => {
  const alvo = e.target && e.target.closest ? e.target : null;
  if (!alvo) return;
  const caixa = alvo.closest('[data-t]');
  const link = alvo.closest('[data-a]');
  const ancora = alvo.closest('a');
  if (caixa || link || ancora) e.preventDefault();
  if (caixa) dioxus.send('t' + caixa.dataset.t);
  else if (link) dioxus.send('a' + link.dataset.a);
};
document.addEventListener('click', aoTocar, true);
window.__leituraOff = () => document.removeEventListener('click', aoTocar, true);
"#;

pub const DESLIGAR_LEITURA: &str = "if (window.__leituraOff) window.__leituraOff();";

/// O que um toque na Leitura pediu (o número indexa `Pagina::alvos` ou `Pagina::tarefas`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toque {
    Alvo(usize),
    Tarefa(usize),
}

/// Lê a mensagem do `SCRIPT_LEITURA` (`a3`, `t0`). Qualquer outra coisa é ignorada.
pub fn interpretar_toque(mensagem: &str) -> Option<Toque> {
    let numero = |resto: &str| {
        (!resto.is_empty() && resto.bytes().all(|b| b.is_ascii_digit()))
            .then(|| resto.parse().ok())
            .flatten()
    };
    match mensagem.split_at_checked(1)? {
        ("a", n) => numero(n).map(Toque::Alvo),
        ("t", n) => numero(n).map(Toque::Tarefa),
        _ => None,
    }
}

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

    #[test]
    fn toques_da_leitura() {
        assert_eq!(interpretar_toque("a0"), Some(Toque::Alvo(0)));
        assert_eq!(interpretar_toque("a12"), Some(Toque::Alvo(12)));
        assert_eq!(interpretar_toque("t3"), Some(Toque::Tarefa(3)));
        for ruim in [
            "",
            "a",
            "t",
            "x1",
            "a-1",
            "a1x",
            "a 1",
            "voltar",
            "ç1",
            "a99999999999999999999",
        ] {
            assert_eq!(interpretar_toque(ruim), None, "{ruim:?}");
        }
    }
}
