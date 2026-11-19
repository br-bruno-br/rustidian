//! Agrupa a digitação em versões "como o Word": nas pausas naturais, nunca tecla a tecla.
//!
//! O [`Agrupador`] é uma função pura sobre eventos: não lê relógio, não grava nada e não conhece o
//! banco. O editor conta os eventos (cada edição, cada "tique" de pausa, salvar, trocar de nota,
//! fechar) com um tempo qualquer em [`Duration`] (por exemplo, `Instant::now() - inicio_do_app`) e
//! o agrupador diz **quando** gravar; quem chama então chama [`crate::Historico::gravar`] com o
//! texto atual. Os testes usam um relógio simulado.
//!
//! # Quando grava
//!
//! Só se o texto mudou desde a última gravação (nunca grava duas vezes o mesmo texto):
//!
//! | Situação                                                                    | [`Motivo`]        |
//! |-----------------------------------------------------------------------------|-------------------|
//! | frase terminada (`.` `!` `?` `…` e depois espaço ou quebra) e 0,8 s sem digitar | `FimDeFrase`     |
//! | Enter (fim de parágrafo) e 0,4 s sem digitar                                | `FimDeParagrafo`  |
//! | 2 s sem digitar                                                             | `Pausa`           |
//! | ~300 caracteres digitados (ou colados) sem gravar                           | `Volume`          |
//! | `Salvou`, `TrocouDeNota` ou `Fechou`                                        | o próprio evento  |
//!
//! Os limiares ficam em [`Ritmo`] e podem ser ajustados.
//!
//! # Exemplos (relógio simulado; nos testes deste módulo)
//!
//! - **Frase digitada e pausa = 1 versão:** digitar "Olá, mundo. " e parar por 0,8 s grava uma versão
//!   (`FimDeFrase`); parar mais 2 s não grava outra, o texto é o mesmo.
//! - **Parágrafo inteiro digitado sem parar = 1 versão por frase:** quem escreve respira no fim de
//!   cada frase (a pausa de 0,8 s basta). Um parágrafo de 3 frases, com esse respiro entre elas,
//!   dá 3 versões. Se não houver pausa nenhuma, entra o limite de volume: 1 versão a cada ~300
//!   caracteres.
//!
//! # Como o editor liga isto
//!
//! 1. Ao abrir uma nota: [`Agrupador::iniciar`] com o texto que está no histórico (a última versão).
//! 2. A cada quadro em que o texto mudou: `Editou { quando, texto, cursor }`. Mandar o mesmo texto
//!    de novo é inofensivo (não reinicia o relógio). O agrupador calcula o blake3 do texto a cada
//!    `Editou` (uns 3 us para 5 KB; 0,6 ms para 1 MB), então vale mandar só quando mudou.
//! 3. A cada quadro (ou num temporizador de ~100 ms): `Pausa { quando }`. Use
//!    [`Agrupador::proximo_prazo`] para pedir o próximo repaint só quando preciso.
//! 4. Se o resultado é `Some(motivo)`: [`crate::Historico::gravar`] com o texto atual (numa linha de
//!    fundo, nunca na da tela). O agrupador já considera o texto gravado.
//! 5. `TrocouDeNota` e `Fechou` gravam o que sobrou e **zeram** o agrupador: ao abrir a próxima
//!    nota, chame `iniciar` de novo.

use std::time::Duration;

/// Os limiares do agrupador. O padrão segue o briefing H1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ritmo {
    /// Pausa depois de uma frase terminada.
    pub pausa_frase: Duration,
    /// Pausa depois de um Enter.
    pub pausa_paragrafo: Duration,
    /// Pausa que grava em qualquer ponto do texto.
    pub pausa_geral: Duration,
    /// Quantos caracteres sem gravar forçam uma versão.
    pub max_caracteres: usize,
}

impl Default for Ritmo {
    fn default() -> Self {
        Self {
            pausa_frase: Duration::from_millis(800),
            pausa_paragrafo: Duration::from_millis(400),
            pausa_geral: Duration::from_secs(2),
            max_caracteres: 300,
        }
    }
}

/// O que aconteceu no editor. `quando` é um tempo qualquer na mesma escala em todos os eventos.
#[derive(Debug, Clone, Copy)]
pub enum Evento<'a> {
    /// O texto mudou. `cursor` é o deslocamento em bytes logo depois da edição (`None` = fim do texto).
    Editou {
        quando: Duration,
        texto: &'a str,
        cursor: Option<usize>,
    },
    /// Passou o tempo (um tique do relógio da interface), sem edição.
    Pausa { quando: Duration },
    /// O usuário salvou (Ctrl+S ou salvamento automático do arquivo).
    Salvou { quando: Duration },
    /// A nota deixou de ser a ativa.
    TrocouDeNota,
    /// O aplicativo (ou a nota) vai fechar.
    Fechou,
}

/// Por que o agrupador mandou gravar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    FimDeFrase,
    FimDeParagrafo,
    Pausa,
    Volume,
    Salvou,
    TrocouDeNota,
    Fechou,
}

type Impressao = [u8; 32];

fn impressao(t: &str) -> Impressao {
    *blake3::hash(t.as_bytes()).as_bytes()
}

/// Decide quando uma sequência de edições vira uma versão do histórico.
#[derive(Debug, Clone)]
pub struct Agrupador {
    ritmo: Ritmo,
    /// Impressão do último texto gravado (ou do texto inicial).
    gravado: Option<Impressao>,
    /// Impressão do texto da última edição.
    atual: Option<Impressao>,
    ultima_edicao: Duration,
    tamanho_anterior: usize,
    fim_frase: bool,
    fim_paragrafo: bool,
    digitados: usize,
}

const FECHAMENTOS: &[char] = &['"', '\'', ')', ']', '}', '”', '’', '»', '*', '_'];

/// O texto antes do cursor termina numa frase completa ("fim. " ou "fim.\n")?
fn termina_frase(antes: &str) -> bool {
    let Some(ultimo) = antes.chars().next_back() else {
        return false;
    };
    if !ultimo.is_whitespace() {
        return false;
    }
    let sem_espaco = antes.trim_end();
    let sem_fecho = sem_espaco.trim_end_matches(FECHAMENTOS);
    matches!(sem_fecho.chars().next_back(), Some('.' | '!' | '?' | '…'))
}

impl Agrupador {
    pub fn novo(ritmo: Ritmo) -> Self {
        Self {
            ritmo,
            gravado: None,
            atual: None,
            ultima_edicao: Duration::ZERO,
            tamanho_anterior: 0,
            fim_frase: false,
            fim_paragrafo: false,
            digitados: 0,
        }
    }

    pub fn ritmo(&self) -> &Ritmo {
        &self.ritmo
    }

    pub fn ajustar_ritmo(&mut self, ritmo: Ritmo) {
        self.ritmo = ritmo;
    }

    /// Diz qual é o texto que já está gravado (a última versão da nota, ao abri-la). Sem isto, a
    /// primeira edição conta como diferente de tudo.
    pub fn iniciar(&mut self, texto: &str) {
        *self = Self {
            gravado: Some(impressao(texto)),
            atual: Some(impressao(texto)),
            tamanho_anterior: texto.len(),
            ..Self::novo(self.ritmo)
        };
    }

    /// Há texto editado que ainda não foi gravado?
    pub fn sujo(&self) -> bool {
        self.atual.is_some() && self.atual != self.gravado
    }

    fn gravou(&mut self) {
        self.gravado = self.atual;
        self.digitados = 0;
    }

    /// Versão curta de [`Agrupador::evento`]: `true` = grave o texto atual agora.
    pub fn deve_gravar(&mut self, e: Evento<'_>) -> bool {
        self.evento(e).is_some()
    }

    /// Processa um evento. `Some(motivo)` = grave o texto atual (o agrupador já o considera gravado).
    pub fn evento(&mut self, e: Evento<'_>) -> Option<Motivo> {
        match e {
            Evento::Editou {
                quando,
                texto,
                cursor,
            } => {
                let h = impressao(texto);
                if self.atual == Some(h) {
                    // Mesmo texto de antes (a interface mandou de novo): não mexe nos relógios.
                    return None;
                }
                let antes = match cursor {
                    Some(c) if c <= texto.len() && texto.is_char_boundary(c) => &texto[..c],
                    _ => texto,
                };
                self.fim_paragrafo = antes.ends_with('\n');
                self.fim_frase = termina_frase(antes);
                self.digitados += texto.len().abs_diff(self.tamanho_anterior).max(1);
                self.tamanho_anterior = texto.len();
                self.atual = Some(h);
                self.ultima_edicao = quando;
                if self.sujo() && self.digitados >= self.ritmo.max_caracteres {
                    self.gravou();
                    return Some(Motivo::Volume);
                }
                None
            }
            Evento::Pausa { quando } => {
                if !self.sujo() {
                    return None;
                }
                let parado = quando.saturating_sub(self.ultima_edicao);
                let motivo = if self.fim_paragrafo && parado >= self.ritmo.pausa_paragrafo {
                    Motivo::FimDeParagrafo
                } else if self.fim_frase && parado >= self.ritmo.pausa_frase {
                    Motivo::FimDeFrase
                } else if parado >= self.ritmo.pausa_geral {
                    Motivo::Pausa
                } else {
                    return None;
                };
                self.gravou();
                Some(motivo)
            }
            Evento::Salvou { .. } => {
                if !self.sujo() {
                    return None;
                }
                self.gravou();
                Some(Motivo::Salvou)
            }
            Evento::TrocouDeNota | Evento::Fechou => {
                let motivo = if matches!(e, Evento::Fechou) {
                    Motivo::Fechou
                } else {
                    Motivo::TrocouDeNota
                };
                let grava = self.sujo();
                *self = Self::novo(self.ritmo);
                grava.then_some(motivo)
            }
        }
    }

    /// Quando (na escala dos eventos) o próximo `Pausa` vai mandar gravar, se nada mais for digitado.
    /// `None` se não há nada a gravar. Serve para pedir o repaint certo, sem tique constante.
    pub fn proximo_prazo(&self) -> Option<Duration> {
        if !self.sujo() {
            return None;
        }
        let mut espera = self.ritmo.pausa_geral;
        if self.fim_frase {
            espera = espera.min(self.ritmo.pausa_frase);
        }
        if self.fim_paragrafo {
            espera = espera.min(self.ritmo.pausa_paragrafo);
        }
        Some(self.ultima_edicao + espera)
    }
}

impl Default for Agrupador {
    fn default() -> Self {
        Self::novo(Ritmo::default())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Digitador simulado: o relógio é um `Duration` que só anda quando o teste manda.
    struct Mesa {
        ag: Agrupador,
        t: Duration,
        texto: String,
        gravacoes: Vec<(Motivo, String)>,
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    impl Mesa {
        fn nova() -> Self {
            Self::com(Ritmo::default())
        }
        fn com(r: Ritmo) -> Self {
            let mut ag = Agrupador::novo(r);
            ag.iniciar("");
            Self {
                ag,
                t: Duration::ZERO,
                texto: String::new(),
                gravacoes: Vec::new(),
            }
        }
        fn registrar(&mut self, m: Option<Motivo>) {
            if let Some(m) = m {
                self.gravacoes.push((m, self.texto.clone()));
            }
        }
        /// Digita `s` com `intervalo` entre as teclas.
        fn digitar(&mut self, s: &str, intervalo: Duration) {
            for c in s.chars() {
                self.t += intervalo;
                self.texto.push(c);
                let m = self.ag.evento(Evento::Editou {
                    quando: self.t,
                    texto: &self.texto,
                    cursor: None,
                });
                self.registrar(m);
            }
        }
        fn apagar_ultimo(&mut self) {
            self.t += ms(100);
            self.texto.pop();
            let m = self.ag.evento(Evento::Editou {
                quando: self.t,
                texto: &self.texto,
                cursor: None,
            });
            self.registrar(m);
        }
        /// Espera `d`, com um tique de pausa a cada 100 ms.
        fn esperar(&mut self, d: Duration) {
            let fim = self.t + d;
            while self.t < fim {
                self.t = (self.t + ms(100)).min(fim);
                let m = self.ag.evento(Evento::Pausa { quando: self.t });
                self.registrar(m);
            }
        }
        fn evento(&mut self, e: Evento<'_>) -> Option<Motivo> {
            let m = self.ag.evento(e);
            self.registrar(m);
            m
        }
    }

    #[test]
    fn frase_digitada_e_pausa_e_uma_versao() {
        let mut m = Mesa::nova();
        m.digitar("Olá, mundo. ", ms(120));
        m.esperar(ms(700));
        assert!(m.gravacoes.is_empty(), "ainda não passou 0,8 s");
        m.esperar(ms(200));
        assert_eq!(
            m.gravacoes,
            vec![(Motivo::FimDeFrase, "Olá, mundo. ".to_string())]
        );
        // Parar mais não grava de novo: o texto é o mesmo.
        m.esperar(Duration::from_secs(10));
        assert_eq!(m.gravacoes.len(), 1);
    }

    #[test]
    fn paragrafo_com_respiro_entre_frases_da_uma_versao_por_frase() {
        let mut m = Mesa::nova();
        for frase in [
            "Primeira frase aqui. ",
            "A segunda vem logo depois! ",
            "E a terceira? ",
        ] {
            m.digitar(frase, ms(110));
            m.esperar(ms(900));
        }
        assert_eq!(m.gravacoes.len(), 3);
        assert!(m.gravacoes.iter().all(|(mo, _)| *mo == Motivo::FimDeFrase));
        assert_eq!(m.gravacoes[0].1, "Primeira frase aqui. ");
        assert_eq!(
            m.gravacoes[2].1,
            "Primeira frase aqui. A segunda vem logo depois! E a terceira? "
        );
    }

    #[test]
    fn ponto_sem_espaco_nao_e_fim_de_frase_ate_2s() {
        let mut m = Mesa::nova();
        m.digitar("Fim.", ms(100));
        m.esperar(ms(1500));
        assert!(
            m.gravacoes.is_empty(),
            "sem espaço depois do ponto, não é fim de frase"
        );
        m.esperar(ms(600));
        assert_eq!(m.gravacoes, vec![(Motivo::Pausa, "Fim.".to_string())]);
    }

    #[test]
    fn meio_de_frase_so_grava_com_2s_de_pausa() {
        let mut m = Mesa::nova();
        m.digitar("uma frase sem fim", ms(100));
        m.esperar(ms(1900));
        assert!(m.gravacoes.is_empty());
        m.esperar(ms(200));
        assert_eq!(m.gravacoes.len(), 1);
        assert_eq!(m.gravacoes[0].0, Motivo::Pausa);
    }

    #[test]
    fn enter_com_pausa_curta_grava_o_paragrafo() {
        let mut m = Mesa::nova();
        m.digitar("Linha sem ponto final\n", ms(100));
        m.esperar(ms(300));
        assert!(m.gravacoes.is_empty());
        m.esperar(ms(200));
        assert_eq!(m.gravacoes.len(), 1);
        assert_eq!(m.gravacoes[0].0, Motivo::FimDeParagrafo);
    }

    #[test]
    fn sem_parar_a_cada_300_caracteres() {
        let mut m = Mesa::nova();
        // 700 caracteres a 100 ms por tecla, sem frase terminada: nenhuma pausa chega a 0,8 s.
        let palavra = "palavra ";
        let texto: String = palavra.repeat(88); // 704 caracteres
        m.digitar(&texto, ms(100));
        let volume = m
            .gravacoes
            .iter()
            .filter(|(mo, _)| *mo == Motivo::Volume)
            .count();
        assert_eq!(volume, 2, "a cada ~300 caracteres: {:?}", m.gravacoes.len());
        assert_eq!(m.gravacoes[0].1.len(), 300);
        assert_eq!(m.gravacoes[1].1.len(), 600);
        m.esperar(Duration::from_secs(3));
        assert_eq!(m.gravacoes.len(), 3);
        assert_eq!(m.gravacoes[2].0, Motivo::Pausa);
        assert_eq!(m.gravacoes[2].1, texto);
    }

    #[test]
    fn colar_um_texto_grande_grava_na_hora() {
        let mut m = Mesa::nova();
        m.t = ms(1000);
        m.texto = "x".repeat(400);
        let r = m.evento(Evento::Editou {
            quando: m.t,
            texto: &"x".repeat(400),
            cursor: None,
        });
        assert_eq!(r, Some(Motivo::Volume));
    }

    #[test]
    fn salvar_trocar_e_fechar_sempre_gravam_se_mudou() {
        let mut m = Mesa::nova();
        m.digitar("abc", ms(100));
        assert_eq!(
            m.evento(Evento::Salvou { quando: m.t }),
            Some(Motivo::Salvou)
        );
        assert_eq!(
            m.evento(Evento::Salvou { quando: m.t }),
            None,
            "mesmo texto"
        );
        m.digitar("d", ms(100));
        assert_eq!(m.evento(Evento::TrocouDeNota), Some(Motivo::TrocouDeNota));
        m.ag.iniciar("outra nota");
        m.texto = "outra nota!".into();
        m.t += ms(100);
        let r = m.ag.evento(Evento::Editou {
            quando: m.t,
            texto: &m.texto,
            cursor: None,
        });
        assert_eq!(r, None);
        assert_eq!(m.evento(Evento::Fechou), Some(Motivo::Fechou));
        assert_eq!(m.evento(Evento::Fechou), None);
        assert_eq!(m.gravacoes.len(), 3);
    }

    #[test]
    fn nunca_grava_duas_vezes_o_mesmo_texto() {
        let mut m = Mesa::nova();
        m.digitar("a", ms(100));
        m.esperar(Duration::from_secs(3));
        assert_eq!(m.gravacoes.len(), 1);
        // Digita "b" e apaga: o texto volta ao que já está gravado.
        m.digitar("b", ms(100));
        m.apagar_ultimo();
        m.esperar(Duration::from_secs(5));
        assert_eq!(m.evento(Evento::Salvou { quando: m.t }), None);
        assert_eq!(m.gravacoes.len(), 1);
    }

    #[test]
    fn editou_repetido_com_o_mesmo_texto_nao_reinicia_o_relogio() {
        let mut m = Mesa::nova();
        m.digitar("Pronto. ", ms(100));
        // A interface manda o mesmo texto a cada quadro enquanto nada muda.
        let fim = m.t + ms(1000);
        let mut gravou = None;
        while m.t < fim {
            m.t += ms(100);
            m.ag.evento(Evento::Editou {
                quando: m.t,
                texto: &m.texto,
                cursor: None,
            });
            gravou = gravou.or(m.ag.evento(Evento::Pausa { quando: m.t }));
        }
        assert_eq!(gravou, Some(Motivo::FimDeFrase));
    }

    #[test]
    fn cursor_no_meio_decide_pelo_que_veio_antes_dele() {
        let mut ag = Agrupador::default();
        ag.iniciar("");
        let texto = "Acabei de escrever isto. e o resto vem depois";
        let c = "Acabei de escrever isto. ".len();
        assert_eq!(
            ag.evento(Evento::Editou {
                quando: ms(100),
                texto,
                cursor: Some(c)
            }),
            None
        );
        assert_eq!(
            ag.evento(Evento::Pausa { quando: ms(950) }),
            Some(Motivo::FimDeFrase)
        );
        // Cursor inválido (no meio de um caractere) cai para o fim do texto, sem pânico.
        let mut ag = Agrupador::default();
        ag.iniciar("");
        ag.evento(Evento::Editou {
            quando: ms(100),
            texto: "ação",
            cursor: Some(2),
        });
    }

    #[test]
    fn aspas_e_parenteses_depois_do_ponto_ainda_fecham_a_frase() {
        for t in [
            "Ele disse \"vamos.\" ",
            "(isso é um aparte.) ",
            "Será? ",
            "Que bom! ",
            "Pois é… ",
        ] {
            assert!(termina_frase(t), "{t:?}");
        }
        for t in ["sem ponto ", "v1.2 ", "3.", "", " ", "Olá"] {
            assert!(!termina_frase(t), "{t:?}");
        }
    }

    #[test]
    fn ritmo_ajustavel_e_proximo_prazo() {
        let r = Ritmo {
            pausa_frase: ms(300),
            pausa_paragrafo: ms(100),
            pausa_geral: ms(1000),
            max_caracteres: 10,
        };
        let mut m = Mesa::com(r);
        assert_eq!(m.ag.proximo_prazo(), None);
        m.digitar("Oi. ", ms(50));
        assert_eq!(m.ag.proximo_prazo(), Some(m.t + ms(300)));
        m.esperar(ms(300));
        assert_eq!(m.gravacoes.len(), 1);
        assert_eq!(m.ag.proximo_prazo(), None);
        m.digitar("abc", ms(50));
        assert_eq!(m.ag.proximo_prazo(), Some(m.t + ms(1000)));
        // Limite de volume menor: 10 caracteres.
        m.digitar("defghijkl", ms(50));
        assert!(m.gravacoes.iter().any(|(mo, _)| *mo == Motivo::Volume));
    }
}
