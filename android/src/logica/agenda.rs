//! Quando rodar um ciclo de sincronização (sem relógio próprio: quem chama passa o `Instant`,
//! então tudo aqui testa sem esperar).
//!
//! Os gatilhos da R7b-3:
//! - **ao abrir o app** (o ciclo já nasce pedido);
//! - **depois de salvar**, com uma folga (`depois_de_salvar`) para não sincronizar a cada tecla:
//!   uma sequência de gravações empurra o ciclo, mas nunca além de `no_maximo` da primeira;
//! - **ao voltar para o app** (a WebView ficou visível), só se o último ciclo começou há mais de
//!   `ao_voltar_minimo` (alternar entre apps não martela o servidor);
//! - **"Sincronizar agora"**, que vale sempre;
//! - **nova tentativa** depois de uma falha de rede, até `tentativas` vezes seguidas.
//!
//! Não há ciclo periódico: o app só sincroniza quando o dono o usa (poupa bateria e rede).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tempos {
    pub depois_de_salvar: Duration,
    pub no_maximo: Duration,
    pub ao_voltar_minimo: Duration,
    pub nova_tentativa: Duration,
    pub tentativas: u32,
}

impl Default for Tempos {
    fn default() -> Self {
        Self {
            depois_de_salvar: Duration::from_secs(5),
            no_maximo: Duration::from_secs(30),
            ao_voltar_minimo: Duration::from_secs(15),
            nova_tentativa: Duration::from_secs(60),
            tentativas: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Agenda {
    tempos: Tempos,
    pedido_agora: bool,
    /// (quando o ciclo da gravação vence, o mais tarde a que pode ser empurrado)
    gravou: Option<(Instant, Instant)>,
    ultimo_inicio: Option<Instant>,
    nova_tentativa: Option<Instant>,
    falhas_seguidas: u32,
}

impl Agenda {
    /// Começa com um ciclo pedido: "ao abrir o app".
    pub fn nova(tempos: Tempos) -> Self {
        Self {
            tempos,
            pedido_agora: true,
            gravou: None,
            ultimo_inicio: None,
            nova_tentativa: None,
            falhas_seguidas: 0,
        }
    }

    /// "Sincronizar agora": roda assim que possível.
    pub fn pedir_agora(&mut self) {
        self.pedido_agora = true;
    }

    /// O app gravou uma nota.
    pub fn gravou(&mut self, agora: Instant) {
        self.gravou = Some(match self.gravou {
            None => (
                agora + self.tempos.depois_de_salvar,
                agora + self.tempos.no_maximo,
            ),
            Some((_, limite)) => ((agora + self.tempos.depois_de_salvar).min(limite), limite),
        });
    }

    /// O app voltou à tela.
    pub fn voltou(&mut self, agora: Instant) {
        let passou = self
            .ultimo_inicio
            .is_none_or(|t| agora.saturating_duration_since(t) >= self.tempos.ao_voltar_minimo);
        if passou {
            self.pedido_agora = true;
        }
    }

    /// Quando o próximo ciclo vence (`None` = nada agendado).
    pub fn proximo(&self, agora: Instant) -> Option<Instant> {
        if self.pedido_agora {
            return Some(agora);
        }
        match (self.gravou, self.nova_tentativa) {
            (Some((a, _)), Some(b)) => Some(a.min(b)),
            (Some((a, _)), None) => Some(a),
            (None, b) => b,
        }
    }

    pub fn devido(&self, agora: Instant) -> bool {
        self.proximo(agora).is_some_and(|t| t <= agora)
    }

    /// Um ciclo começou: o que estava pedido é atendido por ele.
    pub fn comecou(&mut self, agora: Instant) {
        self.pedido_agora = false;
        self.gravou = None;
        self.nova_tentativa = None;
        self.ultimo_inicio = Some(agora);
    }

    /// O ciclo acabou. `falha_de_rede` agenda uma nova tentativa (no máximo `tentativas`
    /// seguidas; depois só um novo gatilho do dono).
    pub fn terminou(&mut self, agora: Instant, falha_de_rede: bool) {
        if falha_de_rede {
            self.falhas_seguidas += 1;
            if self.falhas_seguidas <= self.tempos.tentativas {
                self.nova_tentativa = Some(agora + self.tempos.nova_tentativa);
            }
        } else {
            self.falhas_seguidas = 0;
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn ao_abrir_o_ciclo_ja_esta_pedido() {
        let t0 = Instant::now();
        let a = Agenda::nova(Tempos::default());
        assert!(a.devido(t0));
        assert_eq!(a.proximo(t0), Some(t0));
    }

    #[test]
    fn depois_de_atender_nao_ha_mais_nada_agendado() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        a.terminou(t0, false);
        assert!(!a.devido(t0 + s(1000)));
        assert_eq!(a.proximo(t0), None);
    }

    #[test]
    fn salvar_espera_a_folga_e_uma_rajada_empurra_o_ciclo() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        a.terminou(t0, false);

        a.gravou(t0 + s(100));
        assert!(!a.devido(t0 + s(102)));
        // Outra gravação 4 s depois empurra o vencimento (4 + 5 = 9 s depois da primeira).
        a.gravou(t0 + s(104));
        assert!(!a.devido(t0 + s(106)));
        assert!(!a.devido(t0 + s(108)));
        assert!(a.devido(t0 + s(109)));
    }

    #[test]
    fn rajada_continua_nao_empurra_alem_do_limite() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        // Uma gravação a cada 3 s, sem parar.
        for i in 0..20 {
            a.gravou(t0 + s(100 + 3 * i));
        }
        // A primeira foi em 100 s; o teto é 130 s.
        assert!(a.devido(t0 + s(130)));
        assert!(!a.devido(t0 + s(129)));
    }

    #[test]
    fn voltar_ao_app_so_pede_se_passou_o_minimo() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        a.terminou(t0 + s(1), false);
        a.voltou(t0 + s(5));
        assert!(!a.devido(t0 + s(5)), "voltou cedo demais");
        a.voltou(t0 + s(20));
        assert!(a.devido(t0 + s(20)));
    }

    #[test]
    fn voltar_sem_ciclo_nenhum_ainda_pede() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.pedido_agora = false;
        a.voltou(t0);
        assert!(a.devido(t0));
    }

    #[test]
    fn sincronizar_agora_vale_sempre() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        a.terminou(t0, false);
        a.pedir_agora();
        assert!(a.devido(t0));
    }

    #[test]
    fn falha_de_rede_tenta_de_novo_ate_o_limite() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        let mut agora = t0;
        for tentativa in 1..=3 {
            a.comecou(agora);
            a.terminou(agora, true);
            assert!(!a.devido(agora + s(59)), "tentativa {tentativa}");
            assert!(a.devido(agora + s(60)), "tentativa {tentativa}");
            agora += s(60);
        }
        // A quarta falha seguida não agenda mais nada sozinha.
        a.comecou(agora);
        a.terminou(agora, true);
        assert_eq!(a.proximo(agora), None);
        // Um ciclo bom zera a conta.
        a.pedir_agora();
        a.comecou(agora);
        a.terminou(agora, false);
        a.comecou(agora);
        a.terminou(agora, true);
        assert!(a.devido(agora + s(60)));
    }

    #[test]
    fn comecar_um_ciclo_atende_gravacao_e_nova_tentativa_pendentes() {
        let t0 = Instant::now();
        let mut a = Agenda::nova(Tempos::default());
        a.comecou(t0);
        a.terminou(t0, true);
        a.gravou(t0 + s(2));
        a.pedir_agora();
        a.comecou(t0 + s(3));
        a.terminou(t0 + s(3), false);
        assert_eq!(a.proximo(t0 + s(3)), None);
    }
}
