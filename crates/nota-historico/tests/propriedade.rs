//! Teste de propriedade: uma sequência aleatória de gravações, podas, nomeações, compactações,
//! apagamentos de trecho e reaberturas. Depois de CADA operação, toda versão que o histórico diz
//! ter reconstrói igual ao texto do modelo (comparando o texto e o blake3 gravado), as versões
//! com nome e a mais nova nunca somem e as referências dos pedaços batem com o uso real.
mod gerador;

use gerador::{Sorteio, editar, texto};
use nota_historico::{
    Durabilidade, Historico, NotaId, Opcoes, Origem, ParametrosPedacos, PoliticaRetencao, VersaoId,
    ZstdCodec,
};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DIA: u64 = 86_400;

fn em(s: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(s)
}

#[derive(Default)]
struct Modelo {
    /// versão -> texto esperado
    versoes: BTreeMap<u64, String>,
    nomeadas: Vec<u64>,
    atual: String,
}

fn verificar(h: &Historico, notas: &[NotaId], modelos: &[Modelo]) {
    for (id, m) in notas.iter().zip(modelos) {
        let infos = h.versoes(id).unwrap();
        let ids: Vec<u64> = infos.iter().map(|v| v.id.0).collect();
        let esperados: Vec<u64> = m.versoes.keys().copied().collect();
        assert_eq!(ids, esperados, "ids de {id:?}");
        for v in &infos {
            let t = h.ler(id, v.id).unwrap();
            assert_eq!(&t, &m.versoes[&v.id.0], "texto de {id:?} v{}", v.id.0);
            assert_eq!(v.impressao[..], blake3::hash(t.as_bytes()).as_bytes()[..16]);
            assert_eq!(v.tamanho, t.len());
        }
        for n in &m.nomeadas {
            assert!(ids.contains(n), "versão nomeada {n} sumiu");
        }
    }
    h.conferir_referencias().unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, ..ProptestConfig::default() })]

    #[test]
    fn toda_versao_que_fica_reconstroi_igual(
        semente in any::<u64>(),
        kf in 1u64..9,
        ops in proptest::collection::vec((0u8..11, any::<u64>(), any::<u64>()), 1..70),
    ) {
        let d = tempfile::tempdir().unwrap();
        let caminho = d.path().join("h.redb");
        let opc = Opcoes {
            quadro_chave_a_cada: kf,
            durabilidade: Durabilidade::Adiada,
            pedacos: ParametrosPedacos { minimo: 24, bits_mascara: 4, maximo: 128 },
        };
        let abrir = || Historico::abrir_com(&caminho, opc, Box::new(ZstdCodec::default())).unwrap();
        let mut h = abrir();
        let notas = [NotaId::novo("n0"), NotaId::novo("n1"), NotaId::novo("n2")];
        let mut s = Sorteio::novo(semente);
        let mut modelos: Vec<Modelo> = notas
            .iter()
            .map(|_| Modelo { atual: texto(&mut s, 500), ..Default::default() })
            .collect();
        let mut relogio = 1_000_000_000u64;

        for (tipo, a, b) in ops {
            let i = (a % 3) as usize;
            match tipo {
                0..=5 => {
                    relogio += if a % 7 == 0 { b % (3 * DIA) } else { b % 7200 };
                    let mut r = Sorteio::novo(b);
                    // Às vezes não muda nada (para exercitar o "texto idêntico").
                    if b % 5 != 0 {
                        editar(&mut r, &mut modelos[i].atual, 20);
                    }
                    let origem = [Origem::Editor, Origem::Externa, Origem::Sync, Origem::Restauracao][(b % 4) as usize];
                    let t = modelos[i].atual.clone();
                    let esperado_igual = modelos[i].versoes.values().next_back() == Some(&t);
                    let r = h.gravar(&notas[i], &t, em(relogio), origem).unwrap();
                    prop_assert_eq!(r.is_none(), esperado_igual);
                    if let Some(VersaoId(v)) = r {
                        modelos[i].versoes.insert(v, t);
                    }
                }
                6 => {
                    let agora = relogio + b % (90 * DIA);
                    let antes: Vec<Vec<u64>> = notas.iter().map(|n| h.versoes(n).unwrap().iter().map(|v| v.id.0).collect()).collect();
                    h.podar(em(agora), &PoliticaRetencao::default()).unwrap();
                    for (k, n) in notas.iter().enumerate() {
                        let depois: Vec<u64> = h.versoes(n).unwrap().iter().map(|v| v.id.0).collect();
                        prop_assert!(depois.iter().all(|x| antes[k].contains(x)), "poda não cria versões");
                        if let Some(ultima) = antes[k].last() {
                            prop_assert_eq!(depois.last(), Some(ultima), "a mais nova fica");
                        }
                        modelos[k].versoes.retain(|id, _| depois.contains(id));
                        modelos[k].nomeadas.retain(|id| depois.contains(id));
                    }
                }
                7 => {
                    let ids: Vec<u64> = modelos[i].versoes.keys().copied().collect();
                    if !ids.is_empty() {
                        let v = ids[(b % ids.len() as u64) as usize];
                        h.nomear(&notas[i], VersaoId(v), Some(format!("marco {v}"))).unwrap();
                        modelos[i].nomeadas.push(v);
                    }
                }
                8 => {
                    h.compactar().unwrap();
                }
                9 => {
                    // Apaga um trecho de 5 caracteres do texto atual (que costuma estar em várias versões).
                    let chars: Vec<char> = modelos[i].atual.chars().collect();
                    if chars.len() > 20 {
                        let ini = (b % (chars.len() as u64 - 6)) as usize;
                        let trecho: String = chars[ini..ini + 5].iter().collect();
                        let esperado = modelos[i].versoes.values().filter(|t| t.contains(&trecho)).count();
                        let n = h.apagar_texto_do_historico(&notas[i], &trecho).unwrap();
                        prop_assert_eq!(n, esperado);
                        for t in modelos[i].versoes.values_mut() {
                            *t = t.replace(&trecho, "");
                        }
                    }
                }
                _ => {
                    drop(h);
                    h = abrir();
                }
            }
            verificar(&h, &notas, &modelos);
        }
    }
}
