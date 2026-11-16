//! Gestos de toque para o egui no celular.
//!
//! O egui trata o dedo como um mouse: encostar e arrastar vira "clicar e arrastar", e dentro de um
//! `TextEdit` isso é seleção de texto. No celular o esperado é:
//! - toque curto: põe o cursor;
//! - arrastar: rolar a nota;
//! - toque longo (parado): seleciona a palavra, e arrastar depois estende a seleção.
//!
//! Este filtro roda sobre os eventos brutos (`App::raw_input_hook`), antes do egui. Ele segura o
//! "botão apertado" do dedo até saber qual dos três gestos é, e então reescreve os eventos.
//! Lógica pura, testada no PC. Eventos de mouse (sem `Event::Touch`) passam intactos.

use eframe::egui::{
    Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2,
};

/// Distância (em pontos) que o dedo pode andar e ainda contar como "parado".
const FOLGA: f32 = 10.0;
/// Tempo (segundos) com o dedo parado até virar toque longo.
pub const LONGO: f64 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Estado {
    Livre,
    /// Dedo apoiado, ainda sem decidir. O "apertar" foi retido.
    Pendente { inicio: Pos2, desde: f64 },
    Rolando { ultimo: Pos2 },
    Selecionando,
}

pub struct FiltroToque {
    estado: Estado,
}

impl Default for FiltroToque {
    fn default() -> Self {
        Self {
            estado: Estado::Livre,
        }
    }
}

fn botao(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn rolar(dy: f32) -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, dy),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    }
}

impl FiltroToque {
    /// Há um dedo apoiado esperando virar toque longo (o chamador deve pedir novo quadro).
    pub fn aguardando(&self) -> bool {
        matches!(self.estado, Estado::Pendente { .. })
    }

    /// Reescreve os eventos do quadro. `agora` é o relógio do egui em segundos.
    pub fn filtrar(&mut self, eventos: Vec<Event>, agora: f64) -> Vec<Event> {
        let mut saida = Vec::with_capacity(eventos.len() + 2);
        self.promover_se_longo(agora, &mut saida);
        for e in eventos {
            match &e {
                Event::Touch {
                    phase: TouchPhase::Start,
                    pos,
                    ..
                } => {
                    if self.estado == Estado::Livre {
                        self.estado = Estado::Pendente {
                            inicio: *pos,
                            desde: agora,
                        };
                    }
                    saida.push(e);
                }
                Event::Touch {
                    phase: TouchPhase::Cancel,
                    ..
                } => {
                    self.estado = Estado::Livre;
                    saida.push(e);
                }
                Event::PointerButton { pressed: true, .. }
                    if matches!(self.estado, Estado::Pendente { .. }) =>
                {
                    // retido: só sai se o gesto virar toque curto ou toque longo
                }
                Event::PointerButton {
                    pressed: false,
                    pos,
                    ..
                } => {
                    match self.estado {
                        Estado::Pendente { .. } => {
                            // Toque curto: devolve o "apertar" que foi retido.
                            saida.push(botao(*pos, true));
                            saida.push(e);
                        }
                        Estado::Rolando { .. } => {} // o soltar de uma rolagem não é clique
                        _ => saida.push(e),
                    }
                    self.estado = Estado::Livre;
                }
                Event::PointerMoved(pos) => {
                    match self.estado {
                        Estado::Pendente { inicio, .. } if (*pos - inicio).length() > FOLGA => {
                            saida.push(rolar(pos.y - inicio.y));
                            self.estado = Estado::Rolando { ultimo: *pos };
                        }
                        Estado::Rolando { ultimo } => {
                            saida.push(rolar(pos.y - ultimo.y));
                            self.estado = Estado::Rolando { ultimo: *pos };
                        }
                        _ => {}
                    }
                    saida.push(e);
                }
                _ => saida.push(e),
            }
        }
        saida
    }

    fn promover_se_longo(&mut self, agora: f64, saida: &mut Vec<Event>) {
        if let Estado::Pendente { inicio, desde } = self.estado
            && agora - desde >= LONGO
        {
            // Aperta, solta e aperta de novo: o egui lê como duplo clique (seleciona a palavra) e
            // o dedo, ainda apoiado, estende a seleção se arrastar.
            saida.push(botao(inicio, true));
            saida.push(botao(inicio, false));
            saida.push(botao(inicio, true));
            self.estado = Estado::Selecionando;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{TouchDeviceId, TouchId, pos2};

    fn toque(phase: TouchPhase, pos: Pos2) -> Event {
        Event::Touch {
            device_id: TouchDeviceId(0),
            id: TouchId(1),
            phase,
            pos,
            force: None,
        }
    }

    fn apertou(saida: &[Event]) -> usize {
        saida
            .iter()
            .filter(|e| matches!(e, Event::PointerButton { pressed: true, .. }))
            .count()
    }

    fn rolou(saida: &[Event]) -> f32 {
        saida
            .iter()
            .map(|e| match e {
                Event::MouseWheel { delta, .. } => delta.y,
                _ => 0.0,
            })
            .sum()
    }

    /// Como o egui-winit entrega o início de um toque.
    fn comeco(p: Pos2) -> Vec<Event> {
        vec![
            toque(TouchPhase::Start, p),
            Event::PointerMoved(p),
            botao(p, true),
        ]
    }

    #[test]
    fn mouse_passa_intacto() {
        let mut f = FiltroToque::default();
        let ev = vec![
            Event::PointerMoved(pos2(5.0, 5.0)),
            botao(pos2(5.0, 5.0), true),
            Event::PointerMoved(pos2(50.0, 50.0)),
            botao(pos2(50.0, 50.0), false),
        ];
        let n = ev.len();
        assert_eq!(f.filtrar(ev, 1.0).len(), n);
    }

    #[test]
    fn toque_curto_devolve_o_clique_so_ao_soltar() {
        let mut f = FiltroToque::default();
        let p = pos2(100.0, 200.0);
        let s = f.filtrar(comeco(p), 1.0);
        assert_eq!(apertou(&s), 0, "o apertar fica retido");
        assert!(f.aguardando());
        let s = f.filtrar(vec![toque(TouchPhase::End, p), botao(p, false)], 1.1);
        assert_eq!(apertou(&s), 1);
        assert!(!f.aguardando());
    }

    #[test]
    fn arrastar_rola_e_nao_clica() {
        let mut f = FiltroToque::default();
        let p = pos2(100.0, 200.0);
        f.filtrar(comeco(p), 1.0);
        let s = f.filtrar(vec![Event::PointerMoved(pos2(100.0, 160.0))], 1.05);
        assert_eq!(rolou(&s), -40.0);
        let s = f.filtrar(vec![Event::PointerMoved(pos2(100.0, 130.0))], 1.1);
        assert_eq!(rolou(&s), -30.0);
        let s = f.filtrar(
            vec![toque(TouchPhase::End, p), botao(pos2(100.0, 130.0), false)],
            1.2,
        );
        assert_eq!(apertou(&s), 0);
        assert!(
            !s.iter()
                .any(|e| matches!(e, Event::PointerButton { pressed: false, .. })),
            "o soltar da rolagem também é engolido"
        );
    }

    #[test]
    fn tremida_pequena_nao_vira_rolagem() {
        let mut f = FiltroToque::default();
        let p = pos2(100.0, 200.0);
        f.filtrar(comeco(p), 1.0);
        let s = f.filtrar(vec![Event::PointerMoved(pos2(103.0, 204.0))], 1.1);
        assert_eq!(rolou(&s), 0.0);
        assert!(f.aguardando());
    }

    #[test]
    fn toque_longo_seleciona_a_palavra() {
        let mut f = FiltroToque::default();
        let p = pos2(100.0, 200.0);
        f.filtrar(comeco(p), 1.0);
        let s = f.filtrar(vec![], 1.0 + LONGO + 0.01);
        assert_eq!(apertou(&s), 2, "aperta, solta, aperta: duplo clique");
        assert!(!f.aguardando());
        // arrastar depois do toque longo estende a seleção: nada de rolagem
        let s = f.filtrar(vec![Event::PointerMoved(pos2(160.0, 200.0))], 2.0);
        assert_eq!(rolou(&s), 0.0);
        // e o soltar chega ao egui
        let s = f.filtrar(
            vec![toque(TouchPhase::End, p), botao(pos2(160.0, 200.0), false)],
            2.1,
        );
        assert!(
            s.iter()
                .any(|e| matches!(e, Event::PointerButton { pressed: false, .. }))
        );
    }

    #[test]
    fn cancelar_o_toque_zera_o_estado() {
        let mut f = FiltroToque::default();
        f.filtrar(comeco(pos2(1.0, 1.0)), 1.0);
        f.filtrar(vec![toque(TouchPhase::Cancel, pos2(1.0, 1.0))], 1.1);
        assert!(!f.aguardando());
    }
}
