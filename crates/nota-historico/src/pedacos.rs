//! Pedaços endereçados pelo conteúdo: o texto de um quadro-chave é cortado onde o PRÓPRIO
//! conteúdo manda (hash rolante "Gear", a ideia do FastCDC), e cada pedaço é guardado uma vez só,
//! com a chave blake3 do conteúdo. Uma edição no meio só muda os pedaços ao redor dela; os outros
//! continuam iguais e são reaproveitados entre versões e entre notas.
//!
//! Implementação própria (uns 30 linhas) em vez da crate `fastcdc`: menos dependência, e o corte
//! é determinístico em qualquer plataforma (a tabela Gear sai de uma semente fixa em tempo de
//! compilação). Os pedaços já gravados não dependem destes parâmetros: cada quadro-chave guarda a
//! lista explícita dos hashes, então mudar os parâmetros no futuro não quebra o que existe.

use std::ops::Range;

/// Tamanhos de corte. O padrão mira ~2 KB (mínimo 1 KB, máximo 4 KB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParametrosPedacos {
    /// Nenhum pedaço (exceto o último) fica menor que isto.
    pub minimo: usize,
    /// Depois do mínimo, corta quando os `bits_mascara` bits altos do hash são zero:
    /// o tamanho médio fica em `minimo + 2^bits_mascara`.
    pub bits_mascara: u32,
    /// Corte forçado.
    pub maximo: usize,
}

impl Default for ParametrosPedacos {
    fn default() -> Self {
        Self {
            minimo: 1024,
            bits_mascara: 10,
            maximo: 4096,
        }
    }
}

/// Tabela Gear: 256 números pseudoaleatórios fixos (splitmix64 com semente constante).
const GEAR: [u64; 256] = {
    let mut t = [0u64; 256];
    let mut x: u64 = 0x5EED_0FC0_DE12_3456;
    let mut i = 0;
    while i < 256 {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        t[i] = z ^ (z >> 31);
        i += 1;
    }
    t
};

/// Corta `dados` em faixas contíguas que cobrem tudo. Dados vazios dão nenhuma faixa.
pub fn cortar(dados: &[u8], p: &ParametrosPedacos) -> Vec<Range<usize>> {
    let mut faixas = Vec::with_capacity(dados.len() / (p.minimo + (1 << p.bits_mascara)) + 1);
    let mut ini = 0;
    while ini < dados.len() {
        let fim_max = (ini + p.maximo).min(dados.len());
        let mut fim = fim_max;
        if ini + p.minimo < fim_max {
            let mut h: u64 = 0;
            for (i, &b) in dados[ini + p.minimo..fim_max].iter().enumerate() {
                h = (h << 1).wrapping_add(GEAR[b as usize]);
                if (h >> (64 - p.bits_mascara)) == 0 {
                    fim = ini + p.minimo + i + 1;
                    break;
                }
            }
        }
        faixas.push(ini..fim);
        ini = fim;
    }
    faixas
}

/// Chave de um pedaço: o blake3 do conteúdo.
pub type Chave = [u8; 32];

pub fn chave(conteudo: &[u8]) -> Chave {
    *blake3::hash(conteudo).as_bytes()
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Bytes pseudoaleatórios determinísticos.
    fn ruido(n: usize, mut x: u64) -> Vec<u8> {
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn cobre_tudo_sem_buracos_e_respeita_limites() {
        let p = ParametrosPedacos::default();
        let d = ruido(100_000, 1);
        let f = cortar(&d, &p);
        assert_eq!(f.first().unwrap().start, 0);
        assert_eq!(f.last().unwrap().end, d.len());
        for w in f.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for (i, r) in f.iter().enumerate() {
            assert!(r.len() <= p.maximo);
            if i + 1 < f.len() {
                assert!(r.len() >= p.minimo);
            }
        }
        let media = d.len() / f.len();
        assert!((1500..3500).contains(&media), "média {media}");
    }

    #[test]
    fn vazio_e_pequeno() {
        let p = ParametrosPedacos::default();
        assert!(cortar(b"", &p).is_empty());
        assert_eq!(cortar(b"abc", &p), vec![0..3]);
    }

    #[test]
    fn insercao_no_comeco_preserva_os_pedacos_seguintes() {
        let p = ParametrosPedacos::default();
        let a = ruido(60_000, 2);
        let mut b = b"texto novo no comeco".to_vec();
        b.extend_from_slice(&a);
        let ka: Vec<Chave> = cortar(&a, &p)
            .iter()
            .map(|r| chave(&a[r.clone()]))
            .collect();
        let kb: Vec<Chave> = cortar(&b, &p)
            .iter()
            .map(|r| chave(&b[r.clone()]))
            .collect();
        let iguais = ka.iter().filter(|k| kb.contains(k)).count();
        assert!(
            iguais + 3 >= ka.len(),
            "só {iguais} de {} pedaços sobreviveram",
            ka.len()
        );
    }

    #[test]
    fn deterministico() {
        let p = ParametrosPedacos::default();
        let d = ruido(30_000, 3);
        assert_eq!(cortar(&d, &p), cortar(&d, &p));
    }
}
