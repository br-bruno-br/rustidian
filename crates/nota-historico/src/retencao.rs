//! Retenção: quais versões ficam quando o histórico é podado. Função pura (sem banco e sem relógio:
//! quem chama passa o `agora`), no estilo do Time Machine do Mac.
//!
//! Padrão ([`PoliticaRetencao::default`]):
//! - tudo das últimas 24 h (`tudo_ate`);
//! - 1 versão por intervalo (1 hora) até 7 dias (`intervalo_ate`);
//! - 1 por dia até 30 dias (`por_dia_ate`);
//! - 1 por semana depois disso, para sempre.
//!
//! O intervalo da segunda faixa é configurável (`intervalo`): com 5 minutos, fica a versão mais
//! nova de cada balde de 5 minutos. Com `intervalo` zero nada é removido nessa faixa (guarda tudo).
//!
//! Sempre ficam: a versão mais nova da nota e as que têm nome. As versões com nome não ocupam a
//! vaga do balde (podem sobrar duas no mesmo dia: a nomeada e a representante do dia).
//!
//! Os baldes são fixos no calendário UTC (múltiplos do intervalo desde a época, dia, semana começando na segunda), não
//! relativos à idade. Assim a poda é idempotente: podar de novo, com o mesmo `agora`, não remove
//! nada; e, com o passar do tempo, de cada balde fica a versão mais nova (o "estado final" da hora,
//! do dia ou da semana).

use std::collections::HashSet;
use std::time::Duration;

const HORA_MS: u64 = 3_600_000;
const DIA_MS: u64 = 24 * HORA_MS;

/// Quanto guardar de cada faixa de idade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoliticaRetencao {
    /// Até esta idade, tudo fica.
    pub tudo_ate: Duration,
    /// Tamanho do balde da segunda faixa: dela, fica a versão mais nova de cada intervalo.
    /// Zero = nada sai na segunda faixa.
    pub intervalo: Duration,
    /// Até esta idade, 1 por `intervalo`.
    pub intervalo_ate: Duration,
    /// Até esta idade, 1 por dia. Depois disso, 1 por semana.
    pub por_dia_ate: Duration,
}

impl Default for PoliticaRetencao {
    fn default() -> Self {
        Self {
            tudo_ate: Duration::from_secs(24 * 3600),
            intervalo: Duration::from_secs(3600),
            intervalo_ate: Duration::from_secs(7 * 24 * 3600),
            por_dia_ate: Duration::from_secs(30 * 24 * 3600),
        }
    }
}

/// Uma versão vista pela retenção.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Candidata {
    pub id: u64,
    pub quando_ms: u64,
    pub nomeada: bool,
}

/// Devolve os ids a remover. `versoes` precisa estar em ordem crescente de id (a última é a mais nova).
pub(crate) fn escolher_remocoes(
    versoes: &[Candidata],
    agora_ms: u64,
    p: &PoliticaRetencao,
) -> HashSet<u64> {
    // (faixa, índice do balde) -> id da versão mais nova daquele balde.
    let mut representante: std::collections::HashMap<(u8, u64), u64> = Default::default();
    let mut remover = HashSet::new();
    let ultima = versoes.last().map(|v| v.id);
    let tudo = p.tudo_ate.as_millis() as u64;
    let intervalo = p.intervalo.as_millis() as u64;
    let ate_intervalo = p.intervalo_ate.as_millis() as u64;
    let dia = p.por_dia_ate.as_millis() as u64;
    for v in versoes {
        if v.nomeada || Some(v.id) == ultima {
            continue;
        }
        let idade = agora_ms.saturating_sub(v.quando_ms);
        if idade <= tudo {
            continue;
        }
        let balde = if idade <= ate_intervalo {
            // Intervalo zero: não há balde, a versão fica (e não divide por zero).
            if intervalo == 0 {
                continue;
            }
            (1u8, v.quando_ms / intervalo)
        } else if idade <= dia {
            (2u8, v.quando_ms / DIA_MS)
        } else {
            // A época (1970-01-01) foi uma quinta; +3 faz a semana começar na segunda.
            (3u8, (v.quando_ms / DIA_MS + 3) / 7)
        };
        // Como as versões vêm em ordem de id, a última a cair no balde é a mais nova.
        if let Some(anterior) = representante.insert(balde, v.id) {
            remover.insert(anterior);
        }
    }
    remover
}

#[cfg(test)]
mod testes {
    use super::*;

    const H: u64 = HORA_MS;
    const D: u64 = DIA_MS;

    fn versoes(ts: &[u64]) -> Vec<Candidata> {
        ts.iter()
            .enumerate()
            .map(|(i, &t)| Candidata {
                id: i as u64 + 1,
                quando_ms: t,
                nomeada: false,
            })
            .collect()
    }

    #[test]
    fn tudo_recente_fica() {
        let agora = 100 * D;
        let v = versoes(&[agora - 3 * H, agora - 2 * H, agora - H, agora - 1000]);
        assert!(escolher_remocoes(&v, agora, &PoliticaRetencao::default()).is_empty());
    }

    #[test]
    fn uma_por_hora_na_semana_fica_a_mais_nova_da_hora() {
        let agora = 100 * D;
        // Três versões na mesma hora de há 3 dias, e uma noutra hora.
        let base = agora - 3 * D;
        let base = base - base % H;
        let v = versoes(&[base + 60_000, base + 120_000, base + 180_000, base + H + 5]);
        let r = escolher_remocoes(&v, agora, &PoliticaRetencao::default());
        assert_eq!(r, HashSet::from([1, 2]));
    }

    #[test]
    fn uma_por_dia_no_mes_e_uma_por_semana_depois() {
        let agora = 400 * D;
        let dia = agora - 20 * D;
        let dia = dia - dia % D;
        // Duas no mesmo dia de há 20 dias.
        let semana: Vec<u64> = (0..7).map(|i| agora - 100 * D + i * D).collect();
        let mut ts = vec![dia + H, dia + 5 * H];
        ts.extend(semana);
        ts.sort();
        let v = versoes(&ts);
        let r = escolher_remocoes(&v, agora, &PoliticaRetencao::default());
        // Do dia sobram 2 (a mais nova da nota entra sempre, e a representante do balde é a outra);
        // dos 7 dias seguidos sobra 1 por semana (1 ou 2).
        let sobram = v.len() - r.len();
        assert!((3..=4).contains(&sobram), "sobraram {sobram}");
        assert!(!r.contains(&v.last().unwrap().id));
    }

    #[test]
    fn nomeada_e_a_mais_nova_nunca_saem() {
        let agora = 400 * D;
        let mut v = versoes(&[agora - 100 * D, agora - 100 * D + 1000, agora - 99 * D]);
        v[0].nomeada = true;
        let r = escolher_remocoes(&v, agora, &PoliticaRetencao::default());
        assert!(!r.contains(&1));
        assert!(!r.contains(&3));
    }

    #[test]
    fn idempotente() {
        let agora = 400 * D;
        let ts: Vec<u64> = (0..500).map(|i| agora - 60 * D + i * (D / 7)).collect();
        let v = versoes(&ts);
        let p = PoliticaRetencao::default();
        let r = escolher_remocoes(&v, agora, &p);
        let resto: Vec<Candidata> = v.iter().copied().filter(|c| !r.contains(&c.id)).collect();
        assert!(!r.is_empty());
        assert!(escolher_remocoes(&resto, agora, &p).is_empty());
    }

    #[test]
    fn intervalo_de_cinco_minutos_fica_a_mais_nova_de_cada_balde() {
        let agora = 100 * D;
        let m5 = 5 * 60_000;
        // Há 3 dias: quatro versões em dois baldes de 5 min.
        let base = agora - 3 * D;
        let base = base - base % m5;
        // A quinta é recente (a mais nova da nota nunca sai).
        let v = versoes(&[
            base + 1_000,
            base + 100_000,
            base + m5 + 2_000,
            base + m5 + 200_000,
            agora - 1_000,
        ]);
        let p = PoliticaRetencao {
            intervalo: Duration::from_secs(300),
            ..PoliticaRetencao::default()
        };
        assert_eq!(escolher_remocoes(&v, agora, &p), HashSet::from([1, 3]));
        // Com o balde de 1 hora do padrão, as quatro caem no mesmo balde (agora é hora cheia).
        let r = escolher_remocoes(&v, agora, &PoliticaRetencao::default());
        assert_eq!(r, HashSet::from([1, 2, 3]));
    }

    #[test]
    fn intervalo_zero_nao_remove_nada_na_segunda_faixa() {
        let agora = 100 * D;
        let base = agora - 3 * D;
        let v = versoes(&[base, base + 1, base + 2, base + 3, base + H]);
        let p = PoliticaRetencao {
            intervalo: Duration::ZERO,
            ..PoliticaRetencao::default()
        };
        assert!(escolher_remocoes(&v, agora, &p).is_empty());
    }

    #[test]
    fn padrao_continua_igual_ao_da_h1() {
        let p = PoliticaRetencao::default();
        assert_eq!(p.tudo_ate, Duration::from_secs(24 * 3600));
        assert_eq!(p.intervalo, Duration::from_secs(3600));
        assert_eq!(p.intervalo_ate, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(p.por_dia_ate, Duration::from_secs(30 * 24 * 3600));
        // Mesmo resultado de "1 por hora": os baldes de 1 h do intervalo são os da hora cheia.
        let agora = 100 * D;
        let base = agora - 3 * D;
        let base = base - base % H;
        let v = versoes(&[base + 60_000, base + 120_000, base + H + 5]);
        assert_eq!(escolher_remocoes(&v, agora, &p), HashSet::from([1]));
    }

    #[test]
    fn relogio_atrasado_nao_estoura() {
        // Versão "do futuro" (relógio do aparelho voltou): idade 0, fica.
        let v = versoes(&[1_000_000, 2_000_000]);
        assert!(escolher_remocoes(&v, 10, &PoliticaRetencao::default()).is_empty());
    }
}
