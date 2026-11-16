//! Tradução do estado do teclado do Android (GameTextInput) em eventos de IME do egui.
//!
//! Por quê: o winit 0.30 (usado pelo eframe 0.37) recebe do GameActivity o evento
//! `TextEvent` (texto, seleção e região em composição do Gboard), mas o descarta
//! (`_ => warn!("Unknown android_activity input event")` em
//! `winit-0.30.13/src/platform_impl/android/mod.rs`). Sem isso o egui nunca vê o
//! texto composto, o autocorretor nem as sugestões. Aqui lemos o estado direto do
//! `AndroidApp::text_input_state()` e comparamos com o estado anterior.
//!
//! Lógica pura, sem dependência do Android, para ser testada no PC.

/// O que o Gboard acha que está no campo. Índices em bytes UTF-8 (como o GameTextInput).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EstadoIme {
    pub texto: String,
    /// Região em composição (a palavra sublinhada que o teclado ainda pode trocar).
    pub composicao: Option<(usize, usize)>,
}

/// Evento a entregar ao egui, na ordem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventoIme {
    /// `ImeEvent::Preedit` vazio: some a composição que o egui está mostrando.
    LimparComposicao,
    /// `ImeEvent::DeleteSurrounding { before_chars, after_chars: 0 }`.
    ApagarAntes(usize),
    /// `ImeEvent::Commit(texto)`. Nunca contém `\n`: a quebra de linha vira [`EventoIme::Enter`],
    /// porque o `TextEdit` do egui ignora `Commit("\n")` e só aceita a tecla Enter.
    Confirmar(String),
    /// Tecla Enter (nova linha).
    Enter,
    /// `ImeEvent::Preedit` com o texto ainda em composição.
    Compor(String),
}

/// O GameTextInput devolve os índices em bytes do UTF-8 *modificado* do Java, onde um caractere
/// fora do plano básico (emoji) ocupa 6 bytes (par substituto) e não 4. Converte para o índice
/// em bytes do `String` do Rust.
pub fn mutf8_para_utf8(texto: &str, indice: usize) -> usize {
    let (mut mutf8, mut utf8) = (0usize, 0usize);
    for c in texto.chars() {
        if mutf8 >= indice {
            break;
        }
        mutf8 += if (c as u32) > 0xFFFF { 6 } else { c.len_utf8() };
        utf8 += c.len_utf8();
    }
    utf8
}

/// Separa o texto em (confirmado, em composição). O confirmado é o texto sem a região
/// de composição; índices inválidos viram "sem composição".
fn partes(e: &EstadoIme) -> (String, String) {
    if let Some((i, f)) = e.composicao
        && i < f
        && f <= e.texto.len()
        && e.texto.is_char_boundary(i)
        && e.texto.is_char_boundary(f)
    {
        let mut confirmado = String::with_capacity(e.texto.len() - (f - i));
        confirmado.push_str(&e.texto[..i]);
        confirmado.push_str(&e.texto[f..]);
        return (confirmado, e.texto[i..f].to_owned());
    }
    (e.texto.clone(), String::new())
}

/// Eventos que levam o egui do estado `antes` ao estado `depois`.
pub fn traduzir(antes: &EstadoIme, depois: &EstadoIme) -> Vec<EventoIme> {
    if antes == depois {
        return Vec::new();
    }
    let (conf0, comp0) = partes(antes);
    let (conf1, comp1) = partes(depois);
    if conf0 == conf1 && comp0 == comp1 {
        return Vec::new(); // só mudou a seleção
    }
    let mut eventos = Vec::new();
    if !comp0.is_empty() {
        eventos.push(EventoIme::LimparComposicao);
    }
    let comum = conf0
        .chars()
        .zip(conf1.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let apagar = conf0.chars().count() - comum;
    if apagar > 0 {
        eventos.push(EventoIme::ApagarAntes(apagar));
    }
    let novo: String = conf1.chars().skip(comum).collect();
    for (i, trecho) in novo.split('\n').enumerate() {
        if i > 0 {
            eventos.push(EventoIme::Enter);
        }
        let trecho = trecho.trim_end_matches('\r');
        if !trecho.is_empty() {
            eventos.push(EventoIme::Confirmar(trecho.to_owned()));
        }
    }
    if !comp1.is_empty() {
        eventos.push(EventoIme::Compor(comp1));
    }
    eventos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(texto: &str, comp: Option<(usize, usize)>) -> EstadoIme {
        EstadoIme {
            texto: texto.to_owned(),
            composicao: comp,
        }
    }

    #[test]
    fn digitando_uma_palavra_so_atualiza_a_composicao() {
        let a = e("ca", Some((0, 2)));
        let b = e("caf", Some((0, 3)));
        assert_eq!(
            traduzir(&a, &b),
            vec![
                EventoIme::LimparComposicao,
                EventoIme::Compor("caf".into())
            ]
        );
    }

    #[test]
    fn espaco_confirma_a_palavra_com_acento() {
        // "cafe" em composição vira "café " confirmado (autocorretor).
        let a = e("cafe", Some((0, 4)));
        let b = e("café ", None);
        assert_eq!(
            traduzir(&a, &b),
            vec![
                EventoIme::LimparComposicao,
                EventoIme::Confirmar("café ".into())
            ]
        );
    }

    #[test]
    fn sugestao_troca_palavra_ja_confirmada() {
        let a = e("oi cafe ", None);
        let b = e("oi café ", None);
        assert_eq!(
            traduzir(&a, &b),
            vec![
                EventoIme::ApagarAntes(2),
                EventoIme::Confirmar("é ".into())
            ]
        );
    }

    #[test]
    fn apagar_pelo_teclado_vira_apagar_antes() {
        let a = e("ação ", None);
        let b = e("ação", None);
        assert_eq!(traduzir(&a, &b), vec![EventoIme::ApagarAntes(1)]);
        let c = e("açã", None);
        assert_eq!(traduzir(&b, &c), vec![EventoIme::ApagarAntes(1)]);
    }

    #[test]
    fn acento_composto_conta_caracteres_e_nao_bytes() {
        // "ç" tem 2 bytes: a região de composição vem em bytes.
        let a = e("ação", Some((0, 6)));
        let b = e("ação ", None);
        assert_eq!(
            traduzir(&a, &b),
            vec![
                EventoIme::LimparComposicao,
                EventoIme::Confirmar("ação ".into())
            ]
        );
    }

    #[test]
    fn so_mudou_a_selecao_nao_gera_evento() {
        let a = e("abc", None);
        assert!(traduzir(&a, &a).is_empty());
        let mut b = a.clone();
        b.composicao = Some((1, 1)); // região vazia: sem composição
        assert!(traduzir(&a, &b).is_empty());
    }

    #[test]
    fn indices_invalidos_viram_sem_composicao() {
        let a = e("", None);
        let b = e("ç", Some((0, 1))); // corta o meio do "ç"
        assert_eq!(traduzir(&a, &b), vec![EventoIme::Confirmar("ç".into())]);
        let c = e("ab", Some((0, 9))); // além do fim
        assert_eq!(traduzir(&a, &c), vec![EventoIme::Confirmar("ab".into())]);
    }

    #[test]
    fn composicao_no_meio_mantem_o_confirmado() {
        let a = e("oi mundo", None);
        let b = e("oi mundo", Some((3, 8)));
        // só passou a ser composição: o confirmado "oi " encurtou, o egui reescreve o resto.
        assert_eq!(
            traduzir(&a, &b),
            vec![
                EventoIme::ApagarAntes(5),
                EventoIme::Compor("mundo".into())
            ]
        );
    }

    #[test]
    fn quebra_de_linha_vira_enter() {
        let a = e("oi", None);
        let b = e("oi\n", None);
        assert_eq!(traduzir(&a, &b), vec![EventoIme::Enter]);
        let c = e("oi\nja", None);
        assert_eq!(traduzir(&b, &c), vec![EventoIme::Confirmar("ja".into())]);
        let d = e("oi", None);
        let f = e("oi a\nb", None);
        assert_eq!(
            traduzir(&d, &f),
            vec![
                EventoIme::Confirmar(" a".into()),
                EventoIme::Enter,
                EventoIme::Confirmar("b".into())
            ]
        );
    }

    #[test]
    fn emoji_e_um_caractere_so() {
        let a = e("oi ", None);
        let b = e("oi 😀", None);
        assert_eq!(traduzir(&a, &b), vec![EventoIme::Confirmar("😀".into())]);
        let c = e("oi ", None);
        assert_eq!(traduzir(&b, &c), vec![EventoIme::ApagarAntes(1)]);
    }

    #[test]
    fn indice_do_java_com_emoji_vira_indice_do_rust() {
        // "a😀b": no Java 1 + 6 + 1 bytes; no Rust 1 + 4 + 1.
        let t = "a😀b";
        assert_eq!(mutf8_para_utf8(t, 0), 0);
        assert_eq!(mutf8_para_utf8(t, 1), 1);
        assert_eq!(mutf8_para_utf8(t, 7), 5);
        assert_eq!(mutf8_para_utf8(t, 8), 6);
        assert_eq!(mutf8_para_utf8("ação", 3), 3); // "ç" tem 2 bytes nos dois
    }
}
