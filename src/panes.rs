//! Painéis da área de edição (R15): o modelo da tela dividida, sem tela.
//!
//! As abas continuam todas em `NotesApp::tabs` (cada uma com o seu `Document`, o seu autosave e o
//! seu portão do sync). Este módulo só diz **quais abas cada painel mostra** e **qual está ativa**
//! em cada um. A mesma aba pode aparecer em dois painéis: é uma só nota na memória, desenhada duas
//! vezes, então o que se digita num painel já está no outro e a nota é salva uma vez só.
//!
//! Desenho para N painéis (`MAX_PANES` limita a 2 por enquanto): uma fila de painéis, em linha
//! (`Orientation::Side`, lado a lado) ou em coluna (`Orientation::Stacked`, em cima e embaixo), com
//! um peso por painel. Código nosso, do zero; só o comportamento do Obsidian serviu de régua.

use serde::{Deserialize, Serialize};

/// Quantos painéis a interface cria. O modelo aceita mais; os comandos e a tela, não.
pub const MAX_PANES: usize = 2;
/// Tamanho mínimo de um painel, em pontos, ao arrastar a divisória.
pub const MIN_PANE: f32 = 160.0;
/// Espessura da divisória entre dois painéis, em pontos.
pub const DIVIDER: f32 = 6.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Um painel ao lado do outro ("Dividir à direita").
    #[default]
    Side,
    /// Um painel sobre o outro ("Dividir abaixo").
    Stacked,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Pane {
    /// As abas que o painel lista (por id da aba). Conjunto: a ordem de desenho é a de `tabs`.
    tabs: Vec<u64>,
    active: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panes {
    orientation: Orientation,
    /// Um peso positivo por painel; o tamanho de cada um é o seu peso sobre a soma.
    weights: Vec<f32>,
    panes: Vec<Pane>,
    focus: usize,
}

impl Default for Panes {
    fn default() -> Self {
        Self::new()
    }
}

/// O layout como vai para o disco: sem ids de aba (que mudam a cada abertura), só posições na
/// lista de abas salva ao lado.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub orientation: Orientation,
    pub weights: Vec<f32>,
    pub panes: Vec<SavedPane>,
    pub focus: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPane {
    pub tabs: Vec<usize>,
    #[serde(default)]
    pub active: Option<usize>,
}

impl Panes {
    /// Um painel só: o app se comporta como antes da divisão.
    pub fn new() -> Self {
        Self {
            orientation: Orientation::Side,
            weights: vec![1.0],
            panes: vec![Pane::default()],
            focus: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.panes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    pub fn can_split(&self) -> bool {
        self.panes.len() < MAX_PANES
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// A aba ativa de um painel.
    pub fn active_of(&self, pane: usize) -> Option<u64> {
        self.panes.get(pane).and_then(|pane| pane.active)
    }

    pub fn focused_active(&self) -> Option<u64> {
        self.active_of(self.focus)
    }

    /// As abas que um painel lista, na ordem em que entraram.
    pub fn tabs_of(&self, pane: usize) -> &[u64] {
        self.panes.get(pane).map_or(&[], |pane| &pane.tabs)
    }

    /// Se o painel lista a aba. Com um painel só, ele lista todas.
    pub fn shows(&self, pane: usize, tab: u64) -> bool {
        !self.is_split() || self.tabs_of(pane).contains(&tab)
    }

    /// Em quantos painéis a aba aparece.
    pub fn shown_in(&self, tab: u64) -> usize {
        self.panes
            .iter()
            .filter(|pane| pane.tabs.contains(&tab))
            .count()
    }

    /// O painel que não tem o foco, quando existe (o próximo, em volta).
    pub fn other(&self) -> Option<usize> {
        (self.is_split()).then(|| (self.focus + 1) % self.panes.len())
    }

    pub fn focus_pane(&mut self, pane: usize) -> bool {
        if pane < self.panes.len() && pane != self.focus {
            self.focus = pane;
            true
        } else {
            false
        }
    }

    /// Cria um painel que mostra `tab` (a mesma aba do painel de origem, se for ela) e o foca.
    /// Devolve falso se já há painéis demais.
    pub fn split(&mut self, orientation: Orientation, tab: u64) -> bool {
        if !self.can_split() {
            return false;
        }
        if !self.is_split() {
            self.orientation = orientation;
        }
        self.panes.push(Pane {
            tabs: vec![tab],
            active: Some(tab),
        });
        // Os painéis nascem do mesmo tamanho.
        self.weights = vec![1.0; self.panes.len()];
        self.focus = self.panes.len() - 1;
        true
    }

    /// Faz o painel mostrar a aba (se ainda não mostra), a deixa ativa nele e passa o foco para lá.
    pub fn show_in(&mut self, pane: usize, tab: u64) {
        let Some(target) = self.panes.get_mut(pane) else {
            return;
        };
        if !target.tabs.contains(&tab) {
            target.tabs.push(tab);
        }
        target.active = Some(tab);
        self.focus = pane;
    }

    /// Passa a aba de onde estiver para o painel (arrastar a aba de um painel para outro).
    pub fn move_to(&mut self, pane: usize, tab: u64) {
        if pane >= self.panes.len() {
            return;
        }
        for (index, other) in self.panes.iter_mut().enumerate() {
            if index != pane {
                other.tabs.retain(|id| *id != tab);
            }
        }
        self.show_in(pane, tab);
    }

    /// Tira a aba da lista do painel; a aba em si continua aberta nos outros painéis.
    pub fn unshow(&mut self, pane: usize, tab: u64) {
        if let Some(target) = self.panes.get_mut(pane) {
            target.tabs.retain(|id| *id != tab);
        }
    }

    /// Fecha um painel: as abas dele passam para o vizinho (nenhuma nota é fechada nem perde
    /// edição). Com um painel só não faz nada.
    pub fn close_pane(&mut self, pane: usize) {
        if !self.is_split() || pane >= self.panes.len() {
            return;
        }
        let closed = self.panes.remove(pane);
        self.weights.remove(pane);
        let heir = pane.saturating_sub(1).min(self.panes.len() - 1);
        for tab in closed.tabs {
            if !self.panes[heir].tabs.contains(&tab) {
                self.panes[heir].tabs.push(tab);
            }
        }
        if self.panes[heir].active.is_none() {
            self.panes[heir].active = closed.active;
        }
        self.focus = heir;
        if !self.is_split() {
            self.weights = vec![1.0];
        }
    }

    /// Põe o modelo de acordo com as abas que existem e com a aba ativa do app (`current`), que os
    /// comandos mexem sem saber de painéis:
    /// - aba que sumiu sai de todo painel; a ativa que sumiu passa para uma vizinha do mesmo painel;
    /// - aba que nenhum painel lista (nova, reaberta) vai para o painel com o foco;
    /// - painel que ficou sem abas fecha (o último painel nunca fecha);
    /// - se o app ativou uma aba que só o outro painel mostra, o foco vai para lá.
    ///
    /// Devolve a aba que deve ficar ativa no app.
    pub fn reconcile(&mut self, live: &[u64], current: Option<u64>) -> Option<u64> {
        let lost_focus = self.panes[self.focus]
            .active
            .is_some_and(|active| !live.contains(&active));
        for pane in &mut self.panes {
            let lost_at = pane
                .active
                .filter(|active| !live.contains(active))
                .and_then(|active| pane.tabs.iter().position(|id| *id == active));
            let mut seen = Vec::with_capacity(pane.tabs.len());
            pane.tabs.retain(|id| {
                let keep = live.contains(id) && !seen.contains(id);
                if keep {
                    seen.push(*id);
                }
                keep
            });
            if pane
                .active
                .is_none_or(|active| !pane.tabs.contains(&active))
            {
                let near = lost_at.map(|at| at.min(pane.tabs.len().saturating_sub(1)));
                pane.active = near
                    .and_then(|at| pane.tabs.get(at))
                    .or_else(|| pane.tabs.last())
                    .copied();
            }
        }
        for id in live {
            if !self.panes.iter().any(|pane| pane.tabs.contains(id)) {
                let focused = &mut self.panes[self.focus];
                focused.tabs.push(*id);
                if focused.active.is_none() {
                    focused.active = Some(*id);
                }
            }
        }
        let mut focus_removed = false;
        let mut index = 0;
        while self.panes.len() > 1 && index < self.panes.len() {
            if self.panes[index].tabs.is_empty() {
                self.panes.remove(index);
                self.weights.remove(index);
                match index.cmp(&self.focus) {
                    std::cmp::Ordering::Less => self.focus -= 1,
                    std::cmp::Ordering::Equal => {
                        focus_removed = true;
                        self.focus = self.focus.min(self.panes.len() - 1);
                    }
                    std::cmp::Ordering::Greater => {}
                }
            } else {
                index += 1;
            }
        }
        self.focus = self.focus.min(self.panes.len() - 1);
        if !self.is_split() {
            self.weights = vec![1.0];
        }
        if let Some(current) = current
            && !lost_focus
            && !focus_removed
        {
            if self.panes[self.focus].tabs.contains(&current) {
                self.panes[self.focus].active = Some(current);
            } else if let Some(owner) = self
                .panes
                .iter()
                .position(|pane| pane.active == Some(current))
                .or_else(|| {
                    self.panes
                        .iter()
                        .position(|pane| pane.tabs.contains(&current))
                })
            {
                self.panes[owner].active = Some(current);
                self.focus = owner;
            }
        }
        self.focused_active()
    }

    /// Os tamanhos dos painéis, em pontos, numa área de `total` pontos na direção da fila.
    pub fn sizes(&self, total: f32) -> Vec<f32> {
        let count = self.panes.len();
        if count <= 1 {
            return vec![total.max(0.0)];
        }
        let usable = (total - DIVIDER * (count - 1) as f32).max(0.0);
        let sum: f32 = self.weights.iter().sum();
        self.weights
            .iter()
            .map(|weight| usable * weight / sum)
            .collect()
    }

    /// Arrasta a divisória entre `divider` e `divider + 1` por `delta` pontos, sem deixar painel
    /// nenhum abaixo de `MIN_PANE`.
    pub fn drag(&mut self, divider: usize, delta: f32, total: f32) {
        if divider + 1 >= self.panes.len() || !delta.is_finite() {
            return;
        }
        let usable = total - DIVIDER * (self.panes.len() - 1) as f32;
        if usable <= 2.0 * MIN_PANE {
            return;
        }
        let sum: f32 = self.weights.iter().sum();
        let pair = self.weights[divider] + self.weights[divider + 1];
        let least = MIN_PANE / usable * sum;
        let wanted = self.weights[divider] + delta / usable * sum;
        let first = wanted.clamp(least, (pair - least).max(least));
        self.weights[divider] = first;
        self.weights[divider + 1] = pair - first;
    }

    /// Quanto vale cada painel hoje, para o teste e para a sessão.
    pub fn weights(&self) -> &[f32] {
        &self.weights
    }

    /// O layout para o disco. `position` diz em que lugar da lista de abas salva está a aba; uma
    /// aba que não está lá (nunca acontece com o app em ordem) é ignorada. Com um painel só não há
    /// nada para guardar.
    pub fn save(&self, position: impl Fn(u64) -> Option<usize>) -> Option<Saved> {
        if !self.is_split() {
            return None;
        }
        Some(Saved {
            orientation: self.orientation,
            weights: self.weights.clone(),
            panes: self
                .panes
                .iter()
                .map(|pane| SavedPane {
                    tabs: pane.tabs.iter().filter_map(|id| position(*id)).collect(),
                    active: pane.active.and_then(&position),
                })
                .collect(),
            focus: self.focus,
        })
    }

    /// O layout do disco de volta. `tab_id` dá o id da aba que ficou na posição salva (ou nada,
    /// se aquele arquivo não abriu mais). Um layout inválido ou que ficou com menos de dois painéis
    /// com abas vira `None` (um painel só).
    pub fn load(saved: &Saved, tab_id: impl Fn(usize) -> Option<u64>) -> Option<Self> {
        let count = saved.panes.len();
        if !(2..=MAX_PANES).contains(&count)
            || saved.weights.len() != count
            || saved
                .weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
        let mut panes = Vec::new();
        let mut weights = Vec::new();
        let mut focus = None;
        for (index, (saved_pane, weight)) in saved.panes.iter().zip(&saved.weights).enumerate() {
            let tabs: Vec<u64> = saved_pane.tabs.iter().filter_map(|i| tab_id(*i)).collect();
            if tabs.is_empty() {
                continue;
            }
            if index == saved.focus {
                focus = Some(panes.len());
            }
            let active = saved_pane
                .active
                .and_then(&tab_id)
                .filter(|id| tabs.contains(id))
                .or_else(|| tabs.last().copied());
            panes.push(Pane { tabs, active });
            weights.push(*weight);
        }
        if panes.len() < 2 {
            return None;
        }
        Some(Self {
            orientation: saved.orientation,
            weights,
            focus: focus.unwrap_or(0),
            panes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dois_paineis() -> Panes {
        // Abas 1, 2 e 3 no painel 0; a aba 2 também aparece no painel 1.
        let mut panes = Panes::new();
        panes.reconcile(&[1, 2, 3], Some(2));
        assert!(panes.split(Orientation::Side, 2));
        panes
    }

    #[test]
    fn um_painel_so_lista_todas_as_abas_e_nao_ha_o_que_salvar() {
        let mut panes = Panes::new();
        assert_eq!(panes.reconcile(&[1, 2], Some(2)), Some(2));
        assert!(!panes.is_split());
        assert!(panes.shows(0, 1) && panes.shows(0, 2));
        assert!(panes.save(|id| Some(id as usize)).is_none());
        assert_eq!(panes.other(), None);
    }

    #[test]
    fn dividir_cria_o_segundo_painel_com_a_mesma_aba_e_passa_o_foco() {
        let panes = dois_paineis();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes.focus(), 1);
        assert_eq!(panes.tabs_of(0), &[1, 2, 3]);
        assert_eq!(panes.tabs_of(1), &[2]);
        assert_eq!(panes.shown_in(2), 2);
        assert_eq!(panes.shown_in(1), 1);
        assert!(panes.shows(1, 2) && !panes.shows(1, 1));
        assert_eq!(panes.other(), Some(0));
        assert_eq!(panes.orientation(), Orientation::Side);
        // Já no limite: não cria outro.
        let mut panes = panes;
        assert!(!panes.split(Orientation::Stacked, 1));
        assert_eq!(panes.len(), 2);
    }

    #[test]
    fn aba_nova_cai_no_painel_com_o_foco_e_vira_a_ativa() {
        let mut panes = dois_paineis();
        assert_eq!(panes.reconcile(&[1, 2, 3, 4], Some(4)), Some(4));
        assert_eq!(panes.tabs_of(1), &[2, 4]);
        assert_eq!(panes.tabs_of(0), &[1, 2, 3]);
        assert_eq!(panes.focus(), 1);
    }

    #[test]
    fn ativar_uma_aba_que_so_o_outro_painel_mostra_muda_o_foco() {
        let mut panes = dois_paineis();
        assert_eq!(panes.reconcile(&[1, 2, 3], Some(3)), Some(3));
        assert_eq!(panes.focus(), 0);
        assert_eq!(panes.active_of(0), Some(3));
        // O painel 1 segue com a aba dele.
        assert_eq!(panes.active_of(1), Some(2));
    }

    #[test]
    fn a_aba_nos_dois_paineis_nao_tira_o_foco_do_painel_atual() {
        let mut panes = dois_paineis();
        panes.focus_pane(0);
        assert_eq!(panes.reconcile(&[1, 2, 3], Some(2)), Some(2));
        assert_eq!(panes.focus(), 0, "a aba 2 está nos dois; fica onde estava");
    }

    #[test]
    fn fechar_a_ativa_do_painel_focado_escolhe_vizinha_do_mesmo_painel() {
        let mut panes = dois_paineis();
        panes.show_in(1, 4);
        panes.reconcile(&[1, 2, 3, 4], Some(4));
        // A aba 4 fecha; o app, sem saber de painéis, pulou para a aba 1 (do painel 0).
        let active = panes.reconcile(&[1, 2, 3], Some(1));
        assert_eq!(active, Some(2), "a vizinha no painel 1, não a do painel 0");
        assert_eq!(panes.focus(), 1);
    }

    #[test]
    fn painel_que_perde_a_ultima_aba_fecha_e_o_outro_fica_com_o_foco() {
        let mut panes = dois_paineis();
        panes.show_in(1, 5);
        panes.reconcile(&[1, 2, 3, 5], Some(5));
        panes.unshow(1, 2);
        // A aba 5 fecha: o painel 1 fica sem nada.
        let active = panes.reconcile(&[1, 2, 3], Some(1));
        assert_eq!(panes.len(), 1);
        assert_eq!(panes.focus(), 0);
        // Volta para a aba que o painel 0 já tinha ativa (a 2), não a que o app pulou.
        assert_eq!(active, Some(2));
        assert!(!panes.is_split());
        assert_eq!(panes.weights(), &[1.0]);
    }

    #[test]
    fn fechar_o_painel_junta_as_abas_no_vizinho_sem_fechar_nenhuma() {
        let mut panes = dois_paineis();
        panes.show_in(1, 4);
        panes.reconcile(&[1, 2, 3, 4], Some(4));
        panes.close_pane(1);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes.tabs_of(0), &[1, 2, 3, 4]);
        assert_eq!(panes.reconcile(&[1, 2, 3, 4], Some(4)), Some(4));
        // Com um painel só, fechar não faz nada.
        panes.close_pane(0);
        assert_eq!(panes.len(), 1);
    }

    #[test]
    fn mover_a_aba_para_o_outro_painel_tira_do_de_origem() {
        let mut panes = dois_paineis();
        panes.move_to(1, 3);
        assert_eq!(panes.tabs_of(0), &[1, 2]);
        assert_eq!(panes.tabs_of(1), &[2, 3]);
        assert_eq!(panes.active_of(1), Some(3));
    }

    #[test]
    fn sem_abas_o_unico_painel_continua_e_nada_quebra() {
        let mut panes = Panes::new();
        assert_eq!(panes.reconcile(&[], None), None);
        assert_eq!(panes.len(), 1);
        let mut panes = dois_paineis();
        assert_eq!(panes.reconcile(&[], None), None);
        assert_eq!(panes.len(), 1);
    }

    #[test]
    fn a_divisoria_respeita_o_minimo_e_os_tamanhos_somam_a_area() {
        let mut panes = dois_paineis();
        let total = 1000.0;
        let sizes = panes.sizes(total);
        assert!((sizes[0] + sizes[1] + DIVIDER - total).abs() < 0.01);
        assert!((sizes[0] - sizes[1]).abs() < 0.01);
        panes.drag(0, 200.0, total);
        let sizes = panes.sizes(total);
        assert!(sizes[0] > sizes[1]);
        assert!((sizes[0] + sizes[1] + DIVIDER - total).abs() < 0.01);
        // Puxar longe demais para no mínimo.
        panes.drag(0, 5000.0, total);
        assert!((panes.sizes(total)[1] - MIN_PANE).abs() < 0.01);
        panes.drag(0, -9000.0, total);
        assert!((panes.sizes(total)[0] - MIN_PANE).abs() < 0.01);
        // Área pequena demais ou delta absurdo: ignora.
        let before = panes.weights().to_vec();
        panes.drag(0, f32::NAN, total);
        panes.drag(0, 10.0, 200.0);
        panes.drag(7, 10.0, total);
        assert_eq!(panes.weights(), before);
    }

    #[test]
    fn salvar_e_carregar_devolve_o_mesmo_layout_pelas_posicoes() {
        let ids = [10u64, 20, 30]; // ids de hoje das abas nas posições 0, 1, 2
        let mut panes = Panes::new();
        panes.reconcile(&[10, 20, 30], Some(20));
        panes.split(Orientation::Side, 20);
        panes.show_in(1, 30);
        let saved2 = panes
            .save(|id| ids.iter().position(|x| *x == id))
            .expect("dividido");
        assert_eq!(saved2.panes[0].tabs, vec![0, 1, 2]);
        assert_eq!(saved2.panes[1].tabs, vec![1, 2]);
        assert_eq!(saved2.focus, 1);
        let json = serde_json::to_string(&saved2).unwrap();
        let back: Saved = serde_json::from_str(&json).unwrap();
        let novos = [100u64, 200, 300];
        let carregado = Panes::load(&back, |i| novos.get(i).copied()).expect("dois painéis");
        assert_eq!(carregado.tabs_of(0), &[100, 200, 300]);
        assert_eq!(carregado.tabs_of(1), &[200, 300]);
        assert_eq!(carregado.active_of(1), Some(300));
        assert_eq!(carregado.focus(), 1);
    }

    #[test]
    fn carregar_ignora_abas_perdidas_e_recusa_layout_ruim() {
        let saved = Saved {
            orientation: Orientation::Stacked,
            weights: vec![1.0, 3.0],
            panes: vec![
                SavedPane {
                    tabs: vec![0, 1],
                    active: Some(1),
                },
                SavedPane {
                    tabs: vec![1, 2],
                    active: Some(2),
                },
            ],
            focus: 1,
        };
        // A aba 2 não abriu mais (arquivo apagado): o painel 1 fica só com a 1.
        let panes = Panes::load(&saved, |i| (i != 2).then_some(i as u64 + 10)).expect("ok");
        assert_eq!(panes.orientation(), Orientation::Stacked);
        assert_eq!(panes.tabs_of(1), &[11]);
        assert_eq!(panes.active_of(1), Some(11));
        // Se um painel inteiro some, volta a um painel só.
        assert!(Panes::load(&saved, |i| (i == 0).then_some(10)).is_none());
        // Pesos inválidos, quantidade demais ou de menos.
        let mut ruim = saved.clone();
        ruim.weights = vec![1.0, f32::NAN];
        assert!(Panes::load(&ruim, |i| Some(i as u64)).is_none());
        let mut ruim = saved.clone();
        ruim.weights = vec![1.0, -2.0];
        assert!(Panes::load(&ruim, |i| Some(i as u64)).is_none());
        let mut ruim = saved.clone();
        ruim.weights = vec![1.0];
        assert!(Panes::load(&ruim, |i| Some(i as u64)).is_none());
        let mut ruim = saved.clone();
        ruim.panes.truncate(1);
        ruim.weights.truncate(1);
        assert!(Panes::load(&ruim, |i| Some(i as u64)).is_none());
        // Foco fora do alcance cai no primeiro painel.
        let mut longe = saved;
        longe.focus = 9;
        let panes = Panes::load(&longe, |i| Some(i as u64)).expect("ok");
        assert_eq!(panes.focus(), 0);
    }
}
