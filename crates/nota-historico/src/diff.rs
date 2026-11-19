//! Diferença por palavra, para o "antes e depois" (verde = entrou, vermelho = saiu).
//!
//! Em texto corrido uma linha é um parágrafo inteiro, então a diferença por linha não mostra nada.
//! Aqui o texto vira uma sequência de **palavras, espaços e sinais**, sem perder nenhum byte:
//!
//! - palavra: letras e números (qualquer alfabeto, com acento) e `_`, mais o apóstrofo entre
//!   letras ("d'água" é uma palavra); o hífen é um sinal ("guarda-chuva" = guarda, -, chuva);
//! - espaço: uma sequência de espaços e quebras de linha é um token só;
//! - sinal: cada pontuação, símbolo ou emoji é um token, e as marcas que o acompanham (acento
//!   combinante, seletor de variação, tom de pele, `U+200D` e o caractere que ele junta) ficam
//!   grudadas nele, para não partir "é" escrito como `e` + `U+0301` nem uma família de emoji.
//!
//! Garantias (testadas): juntando os trechos `Igual` e `Saiu` volta-se ao texto de antes; juntando
//! `Igual` e `Entrou`, ao de depois. Trechos vizinhos do mesmo tipo são fundidos.
//!
//! O algoritmo é o Histogram do `imara-diff` (o mesmo do app em `src/recovery.rs`): sem crate nova.

use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

/// O que aconteceu com um trecho.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    /// Está nos dois textos.
    Igual,
    /// Só está no texto de depois.
    Entrou,
    /// Só estava no texto de antes.
    Saiu,
}

/// Um trecho contíguo do resultado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trecho {
    pub tipo: Tipo,
    pub texto: String,
}

fn e_combinante(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F      // acentos combinantes
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF    // inclui o keycap U+20E3
        | 0xFE00..=0xFE0F    // seletores de variação (emoji)
        | 0xFE20..=0xFE2F
        | 0x200D             // junção de largura zero (emoji compostos)
        | 0x1F3FB..=0x1F3FF  // tons de pele
        | 0xE0020..=0xE007F) // etiquetas (bandeiras)
}

fn e_palavra(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Corta o texto em palavras, espaços e sinais. Concatenar os tokens devolve o texto original.
pub fn tokenizar(s: &str) -> Vec<&str> {
    let mut tokens = Vec::with_capacity(s.len() / 4 + 1);
    let mut it = s.char_indices().peekable();
    while let Some((ini, c)) = it.next() {
        let mut fim = ini + c.len_utf8();
        if e_palavra(c) {
            while let Some(&(i, n)) = it.peek() {
                let junta = e_palavra(n) || e_combinante(n) || {
                    // Apóstrofo entre duas letras: d'água, it's.
                    (n == '\'' || n == '’')
                        && s[i + n.len_utf8()..]
                            .chars()
                            .next()
                            .is_some_and(char::is_alphanumeric)
                };
                if !junta {
                    break;
                }
                fim = i + n.len_utf8();
                it.next();
            }
        } else if c.is_whitespace() {
            while let Some(&(i, n)) = it.peek() {
                if !n.is_whitespace() {
                    break;
                }
                fim = i + n.len_utf8();
                it.next();
            }
        } else {
            let mut ultimo = c;
            while let Some(&(i, n)) = it.peek() {
                // Marcas combinantes grudam; depois de uma junção (U+200D) vem o próximo emoji.
                if e_combinante(n) || ultimo == '\u{200D}' {
                    fim = i + n.len_utf8();
                    ultimo = n;
                    it.next();
                } else {
                    break;
                }
            }
        }
        tokens.push(&s[ini..fim]);
    }
    tokens
}

struct Palavras<'a>(&'a str);

impl<'a> TokenSource for Palavras<'a> {
    type Token = &'a str;
    type Tokenizer = std::vec::IntoIter<&'a str>;

    fn tokenize(&self) -> Self::Tokenizer {
        tokenizar(self.0).into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        (self.0.len() / 5) as u32
    }
}

fn empurrar(saida: &mut Vec<Trecho>, tipo: Tipo, texto: &str) {
    if texto.is_empty() {
        return;
    }
    match saida.last_mut() {
        Some(u) if u.tipo == tipo => u.texto.push_str(texto),
        _ => saida.push(Trecho {
            tipo,
            texto: texto.to_owned(),
        }),
    }
}

/// Diferença por palavra entre `antes` e `depois`.
///
/// Custo: linear no número de palavras na prática (o `imara-diff` tem limites contra o pior caso);
/// uma nota de 5 KB leva dezenas de microssegundos.
pub fn diff_palavras(antes: &str, depois: &str) -> Vec<Trecho> {
    let input = InternedInput::new(Palavras(antes), Palavras(depois));
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_no_heuristic(&input);
    let mut saida = Vec::new();
    let mut pos = 0usize;
    let texto = |ts: &[imara_diff::Token]| -> String {
        ts.iter().map(|t| input.interner[*t]).collect::<String>()
    };
    for h in diff.hunks() {
        let (b0, b1) = (h.before.start as usize, h.before.end as usize);
        let (a0, a1) = (h.after.start as usize, h.after.end as usize);
        empurrar(&mut saida, Tipo::Igual, &texto(&input.before[pos..b0]));
        empurrar(&mut saida, Tipo::Saiu, &texto(&input.before[b0..b1]));
        empurrar(&mut saida, Tipo::Entrou, &texto(&input.after[a0..a1]));
        pos = b1;
    }
    empurrar(&mut saida, Tipo::Igual, &texto(&input.before[pos..]));
    saida
}

#[cfg(test)]
mod testes {
    use super::*;

    fn t(tipo: Tipo, s: &str) -> Trecho {
        Trecho {
            tipo,
            texto: s.to_owned(),
        }
    }

    /// As duas garantias: Igual+Saiu = antes, Igual+Entrou = depois.
    fn conferir(antes: &str, depois: &str) -> Vec<Trecho> {
        let d = diff_palavras(antes, depois);
        let a: String = d
            .iter()
            .filter(|x| x.tipo != Tipo::Entrou)
            .map(|x| x.texto.as_str())
            .collect();
        let b: String = d
            .iter()
            .filter(|x| x.tipo != Tipo::Saiu)
            .map(|x| x.texto.as_str())
            .collect();
        assert_eq!(a, antes);
        assert_eq!(b, depois);
        for w in d.windows(2) {
            assert_ne!(w[0].tipo, w[1].tipo, "trechos vizinhos iguais não foram fundidos");
        }
        d
    }

    #[test]
    fn tokeniza_palavras_espacos_e_sinais() {
        assert_eq!(
            tokenizar("Olá, mundo!  Tudo bem?\n"),
            vec!["Olá", ",", " ", "mundo", "!", "  ", "Tudo", " ", "bem", "?", "\n"]
        );
        assert_eq!(tokenizar("guarda-chuva d'água"), vec!["guarda", "-", "chuva", " ", "d'água"]);
        assert_eq!(tokenizar(""), Vec::<&str>::new());
    }

    #[test]
    fn tokeniza_acentos_combinantes_e_emoji_sem_partir() {
        // "café" com e + U+0301 (decomposto) continua uma palavra.
        let nfd = "cafe\u{301} bom";
        assert_eq!(tokenizar(nfd), vec!["cafe\u{301}", " ", "bom"]);
        // Emoji com tom de pele e família com junção.
        assert_eq!(tokenizar("👍🏽 ok"), vec!["👍🏽", " ", "ok"]);
        let familia = "👨\u{200D}👩\u{200D}👧";
        assert_eq!(tokenizar(familia), vec![familia]);
        assert_eq!(tokenizar("❤\u{FE0F}!"), vec!["❤\u{FE0F}", "!"]);
        // Concatenar volta ao original em qualquer caso.
        for s in [nfd, familia, "ação 😀 coração…", "日本語のノート。"] {
            assert_eq!(tokenizar(s).concat(), s);
        }
    }

    #[test]
    fn troca_de_uma_palavra() {
        let d = conferir("Eu gosto de café com leite.", "Eu gosto de chá com leite.");
        assert_eq!(
            d,
            vec![
                t(Tipo::Igual, "Eu gosto de "),
                t(Tipo::Saiu, "café"),
                t(Tipo::Entrou, "chá"),
                t(Tipo::Igual, " com leite."),
            ]
        );
    }

    #[test]
    fn insercao_e_remocao() {
        let d = conferir("a casa é velha", "a casa é muito velha");
        assert_eq!(
            d,
            vec![
                t(Tipo::Igual, "a casa é "),
                t(Tipo::Entrou, "muito "),
                t(Tipo::Igual, "velha"),
            ]
        );
        let d = conferir("a casa é muito velha", "a casa é velha");
        assert_eq!(d.iter().filter(|x| x.tipo == Tipo::Saiu).count(), 1);
    }

    #[test]
    fn acentos_pt_br_nao_viram_palavras_diferentes_por_engano() {
        let d = conferir("ação e reação", "ação e coração");
        assert!(d.iter().any(|x| x.tipo == Tipo::Saiu && x.texto == "reação"));
        assert!(d.iter().any(|x| x.tipo == Tipo::Entrou && x.texto == "coração"));
        // Normalização diferente (NFC x NFD) aparece como mudança, sem partir a palavra.
        let d = conferir("café", "cafe\u{301}");
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn emoji_entra_e_sai_inteiro() {
        let d = conferir("bom dia 😀", "bom dia 👍🏽 😀");
        assert!(d.iter().any(|x| x.tipo == Tipo::Entrou && x.texto.contains("👍🏽")));
        let d = conferir("família 👨\u{200D}👩\u{200D}👧 feliz", "família feliz");
        let saiu: String = d
            .iter()
            .filter(|x| x.tipo == Tipo::Saiu)
            .map(|x| x.texto.as_str())
            .collect();
        assert!(saiu.contains("👨\u{200D}👩\u{200D}👧"));
    }

    #[test]
    fn iguais_vazios_e_totalmente_diferentes() {
        assert_eq!(conferir("", ""), vec![]);
        assert_eq!(conferir("igual", "igual"), vec![t(Tipo::Igual, "igual")]);
        assert_eq!(conferir("", "novo"), vec![t(Tipo::Entrou, "novo")]);
        assert_eq!(conferir("velho", ""), vec![t(Tipo::Saiu, "velho")]);
        let d = conferir("um dois", "três quatro");
        assert!(d.iter().all(|x| x.tipo != Tipo::Igual || x.texto == " "));
    }

    #[test]
    fn pontuacao_e_espacos_sao_preservados() {
        conferir("Olá,  mundo!\n\nFim.", "Olá, mundo!\nFim?");
        conferir("  espaços   no começo", "espaços no começo  ");
    }

    #[test]
    fn nota_grande_e_rapida() {
        let a: String = (0..2000).map(|i| format!("palavra{} ", i % 97)).collect();
        let mut b = a.clone();
        b.insert_str(5000, "inserida aqui ");
        let ini = std::time::Instant::now();
        let d = conferir(&a, &b);
        assert!(ini.elapsed() < std::time::Duration::from_millis(500));
        assert_eq!(d.iter().filter(|x| x.tipo == Tipo::Entrou).count(), 1);
    }
}
