//! Teste de propriedade: 3 ou 4 aparelhos simulados contra um `RemotoMemoria`, com criar,
//! editar, apagar e renomear aleatórios, ciclos intercalados, quedas de rede antes e depois de
//! cada chamada, quedas de processo no meio da aplicação, reinícios e edições do usuário no
//! meio de um ciclo. No fim, ciclos sem falha até estabilizar.
//!
//! Invariantes:
//! (a) todos os cofres ficam iguais em conteúdo;
//! (b) todo texto que o usuário escreveu e não sobrescreveu ele mesmo está num arquivo vivo
//!     ou numa `.trash/` de algum aparelho (a sincronização não perdeu nada);
//! (c) o número de cópias de conflito estabiliza (os ciclos param de agir).
//!
//! `PROPTEST_CASES=N` muda o número de casos (padrão: 256).

mod comum;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use comum::{Aparelho, Mundo};
use nota_sync::ErroSync;
use nota_sync::cofre;
use nota_sync::estado::Intencao;
use nota_sync::memoria::{Falha, Quando};
use proptest::prelude::*;

const CAMINHOS: [&str; 6] = [
    "a.md",
    "b.md",
    "c.md",
    "pasta/d.md",
    "pasta/e.md",
    "pasta/sub/f.md",
];

#[derive(Debug, Clone, Copy)]
enum Meio {
    Editar(usize),
    Criar(usize),
    Apagar(usize),
}

#[derive(Debug, Clone)]
enum Op {
    Criar {
        d: usize,
        p: usize,
    },
    Editar {
        d: usize,
        p: usize,
    },
    Apagar {
        d: usize,
        p: usize,
    },
    Renomear {
        d: usize,
        de: usize,
        para: usize,
    },
    Ciclo {
        d: usize,
        falhas: Vec<(usize, bool)>,
        queda: Option<(usize, bool)>,
        meio: Option<(usize, Meio)>,
        efeito: Option<(usize, Alvo)>,
    },
    Reiniciar {
        d: usize,
    },
}

/// O que o usuário faz, no meio de um ciclo, com o arquivo que a próxima intenção vai tocar.
#[derive(Debug, Clone, Copy)]
enum Alvo {
    Editar,
    Apagar,
}

fn alvo() -> impl Strategy<Value = Alvo> {
    prop_oneof![Just(Alvo::Editar), Just(Alvo::Apagar)]
}

fn caminho_da(i: &Intencao) -> Option<&str> {
    match i {
        Intencao::Enviar { caminho, .. }
        | Intencao::Baixar { caminho, .. }
        | Intencao::ApagarLocal { caminho, .. }
        | Intencao::GravarConflito { caminho, .. } => Some(caminho),
        Intencao::BaixarComo { nome, .. } => Some(nome),
        Intencao::EnviarLapide { caminho, .. } => caminho.as_deref(),
    }
}

fn meio() -> impl Strategy<Value = Meio> {
    prop_oneof![
        (0..CAMINHOS.len()).prop_map(Meio::Editar),
        (0..CAMINHOS.len()).prop_map(Meio::Criar),
        (0..CAMINHOS.len()).prop_map(Meio::Apagar),
    ]
}

fn op(n: usize) -> impl Strategy<Value = Op> {
    let np = CAMINHOS.len();
    prop_oneof![
        3 => (0..n, 0..np).prop_map(|(d, p)| Op::Criar { d, p }),
        3 => (0..n, 0..np).prop_map(|(d, p)| Op::Editar { d, p }),
        2 => (0..n, 0..np).prop_map(|(d, p)| Op::Apagar { d, p }),
        1 => (0..n, 0..np, 0..np).prop_map(|(d, de, para)| Op::Renomear { d, de, para }),
        5 => (
            0..n,
            proptest::collection::vec((0usize..8, any::<bool>()), 0..3),
            proptest::option::of((0usize..5, any::<bool>())),
            proptest::option::of((0usize..6, meio())),
            proptest::option::of((0usize..6, alvo())),
        )
            .prop_map(|(d, falhas, queda, meio, efeito)| Op::Ciclo { d, falhas, queda, meio, efeito }),
        1 => (0..n).prop_map(|d| Op::Reiniciar { d }),
    ]
}

fn cenario() -> impl Strategy<Value = (usize, Vec<Op>)> {
    (3usize..=4).prop_flat_map(|n| (Just(n), proptest::collection::vec(op(n), 5..40)))
}

/// O que o "usuário" escreveu e o que ele mesmo sobrescreveu.
#[derive(Default)]
struct Modelo {
    contador: u64,
    escritos: BTreeSet<String>,
    superados: BTreeSet<String>,
}

type Compartilhado = Arc<Mutex<Modelo>>;

fn user_escrever(dir: &Path, modelo: &Compartilhado, dev: usize, rel: &str) {
    let mut m = modelo.lock().unwrap();
    m.contador += 1;
    let token = format!("<t{} d{dev}>", m.contador);
    let p = dir.join(rel);
    if let Ok(velho) = std::fs::read_to_string(&p) {
        m.superados.insert(velho);
    }
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, &token).unwrap();
    m.escritos.insert(token);
}

fn user_apagar(dir: &Path, rel: &str) {
    if dir.join(rel).is_file() {
        cofre::mover_para_lixeira(dir, rel, "usuario").unwrap();
    }
}

fn user_renomear(dir: &Path, modelo: &Compartilhado, de: &str, para: &str) {
    let Ok(conteudo) = std::fs::read_to_string(dir.join(de)) else {
        return;
    };
    if de == para {
        return;
    }
    cofre::mover_para_lixeira(dir, de, "usuario").unwrap();
    let p = dir.join(para);
    if let Ok(velho) = std::fs::read_to_string(&p) {
        modelo.lock().unwrap().superados.insert(velho);
    }
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, conteudo).unwrap();
}

fn aplicar_meio(dir: &Path, modelo: &Compartilhado, dev: usize, m: Meio) {
    match m {
        Meio::Editar(p) | Meio::Criar(p) => user_escrever(dir, modelo, dev, CAMINHOS[p]),
        Meio::Apagar(p) => user_apagar(dir, CAMINHOS[p]),
    }
}

fn ciclo_com_falhas(
    ap: &mut Aparelho,
    dev: usize,
    modelo: &Compartilhado,
    falhas: &[(usize, bool)],
    queda: Option<(usize, bool)>,
    meio: Option<(usize, Meio)>,
    efeito: Option<(usize, Alvo)>,
) {
    ap.sinc.remoto().injetar(
        falhas
            .iter()
            .map(|&(n, depois)| Falha {
                n,
                quando: if depois {
                    Quando::Depois
                } else {
                    Quando::Antes
                },
            })
            .collect(),
    );
    if let Some((n, depois)) = queda {
        ap.sinc.injetar_queda(n, depois);
    }
    if let Some((k, m)) = meio {
        let dir: PathBuf = ap.cofre.path().to_path_buf();
        let modelo = modelo.clone();
        let mut chamadas = 0usize;
        ap.sinc.remoto().definir_gancho(Some(Box::new(move |_| {
            if chamadas == k {
                aplicar_meio(&dir, &modelo, dev, m);
            }
            chamadas += 1;
        })));
    }
    if let Some((k, m)) = efeito {
        let dir: PathBuf = ap.cofre.path().to_path_buf();
        let modelo = modelo.clone();
        ap.sinc.definir_gancho_efeito(Some(Box::new(move |n, i| {
            if n != k {
                return;
            }
            if let Some(c) = caminho_da(i) {
                match m {
                    Alvo::Editar => user_escrever(&dir, &modelo, dev, c),
                    Alvo::Apagar => user_apagar(&dir, c),
                }
            }
        })));
    }
    let r = ap.ciclo();
    ap.sinc.definir_gancho_efeito(None);
    ap.sinc.remoto().definir_gancho(None);
    ap.sinc.remoto().injetar(vec![]);
    ap.sinc.limpar_queda();
    match r {
        Ok(_) | Err(ErroSync::Remoto(_)) | Err(ErroSync::Interrompido) => {}
        Err(e) => panic!("erro inesperado no ciclo de {}: {e:?}", ap.nome),
    }
}

fn rodar(n: usize, ops: &[Op]) -> Result<(), TestCaseError> {
    let mundo = Mundo::novo();
    let mut aps: Vec<Aparelho> = (0..n).map(|i| mundo.aparelho(&format!("ap{i}"))).collect();
    let modelo: Compartilhado = Arc::new(Mutex::new(Modelo::default()));

    for o in ops {
        match o {
            Op::Criar { d, p } | Op::Editar { d, p } => {
                let dir = aps[*d].cofre.path().to_path_buf();
                let existe = dir.join(CAMINHOS[*p]).is_file();
                // "Editar" só mexe em arquivo que existe; "Criar" cria ou sobrescreve.
                if matches!(o, Op::Criar { .. }) || existe {
                    user_escrever(&dir, &modelo, *d, CAMINHOS[*p]);
                }
            }
            Op::Apagar { d, p } => user_apagar(aps[*d].cofre.path(), CAMINHOS[*p]),
            Op::Renomear { d, de, para } => {
                let dir = aps[*d].cofre.path().to_path_buf();
                user_renomear(&dir, &modelo, CAMINHOS[*de], CAMINHOS[*para]);
            }
            Op::Ciclo {
                d,
                falhas,
                queda,
                meio,
                efeito,
            } => ciclo_com_falhas(&mut aps[*d], *d, &modelo, falhas, *queda, *meio, *efeito),
            Op::Reiniciar { d } => aps[*d].reiniciar(),
        }
    }

    // Sem falhas: ciclos até todos os aparelhos ficarem quietos.
    let mut quieto_em = None;
    for rodada in 0..40 {
        let mut quieto = true;
        for ap in aps.iter_mut() {
            let r = ap
                .ciclo()
                .map_err(|e| TestCaseError::fail(format!("ciclo final de {}: {e:?}", ap.nome)))?;
            quieto &= r.acoes == 0;
        }
        if quieto {
            quieto_em = Some(rodada);
            break;
        }
    }
    prop_assert!(quieto_em.is_some(), "(c) não estabilizou em 40 rodadas");

    // (c) mais três rodadas e nada muda (nem o número de conflitos).
    let antes: Vec<usize> = aps.iter().map(|a| a.conflitos().len()).collect();
    for _ in 0..3 {
        for ap in aps.iter_mut() {
            let r = ap
                .ciclo()
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(r.acoes, 0, "ciclo agiu depois de estabilizar");
        }
    }
    let depois: Vec<usize> = aps.iter().map(|a| a.conflitos().len()).collect();
    prop_assert_eq!(&antes, &depois);

    // (a) cofres iguais.
    let referencia = aps[0].vivos();
    for ap in &aps[1..] {
        prop_assert_eq!(&ap.vivos(), &referencia, "(a) {} difere de ap0", ap.nome);
    }

    // (b) nenhum texto do usuário se perdeu.
    let mut presentes: BTreeSet<String> = BTreeSet::new();
    for ap in &aps {
        presentes.extend(ap.vivos().into_values());
        presentes.extend(ap.lixeira());
    }
    let m = modelo.lock().unwrap();
    for t in m.escritos.difference(&m.superados) {
        prop_assert!(
            presentes.contains(t),
            "(b) o texto {t} sumiu (nem arquivo vivo nem .trash)"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        max_shrink_iters: 400,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn aparelhos_convergem_sem_perder_texto((n, ops) in cenario()) {
        rodar(n, &ops)?;
    }
}
