//! Medição da régua do H1 (espaço, abrir e gravar). Não roda no `cargo test` normal.
//!
//! ```text
//! cargo test --release -p nota-historico --test medicao -- --ignored --nocapture
//! ```
//!
//! Variáveis opcionais para experimentar: `PEDACOS=minimo,bits,maximo`, `KF=quadro_chave_a_cada`
//! e `SO1=1` (só o cenário de 1 nota).
//!
//! Cenário: nota sintética de 5 KB em português, 1.000 edições de ~50 caracteres (inserir, apagar
//! e trocar em posição aleatória, semente fixa), uma versão por edição. A régua do briefing:
//! espaço < 200 KB, abrir qualquer versão < 5 ms, gravar uma versão < 1 ms em média.
mod gerador;

use gerador::*;
use nota_historico::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn em(s: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(s)
}

struct Resultado {
    arquivo_sem_compactar: u64,
    arquivo: u64,
    registros: u64,
    bytes_pedacos: u64,
    pedacos: usize,
    gravar_medio: Duration,
    gravar_pior: Duration,
    gravar_p50: Duration,
    gravar_p99: Duration,
    abrir_medio: Duration,
    abrir_pior: Duration,
    abrir_pior_bruto: Duration,
}

#[derive(Clone, Copy)]
enum Fluxos {
    /// Cada nota recebe edições diferentes (mesmo texto inicial).
    Independentes,
    /// As notas são cópias do mesmo fluxo, cada uma com uma marca no fim.
    Copias,
}

fn rodar(durab: Durabilidade, n_notas: usize, edicoes: usize, fluxos: Fluxos) -> Resultado {
    let d = tempfile::tempdir().unwrap();
    let mut opc = Opcoes {
        durabilidade: durab,
        ..Opcoes::default()
    };
    if let Ok(v) = std::env::var("PEDACOS") {
        let n: Vec<usize> = v.split(',').map(|x| x.parse().unwrap()).collect();
        opc.pedacos = ParametrosPedacos {
            minimo: n[0],
            bits_mascara: n[1] as u32,
            maximo: n[2],
        };
    }
    if let Ok(v) = std::env::var("KF") {
        opc.quadro_chave_a_cada = v.parse().unwrap();
    }
    let mut h = Historico::abrir_com(
        &d.path().join("h.redb"),
        opc,
        Box::new(ZstdCodec::default()),
    )
    .unwrap();
    let mut s = Sorteio::novo(42);
    let base = texto(&mut s, 5000);
    let mut atuais: Vec<String> = vec![base; n_notas];
    let mut sorteios: Vec<Sorteio> = (0..n_notas)
        .map(|k| {
            Sorteio::novo(match fluxos {
                Fluxos::Independentes => 100 + k as u64,
                Fluxos::Copias => 100,
            })
        })
        .collect();
    let (mut soma, mut pior, mut cnt) = (Duration::ZERO, Duration::ZERO, 0u32);
    let mut tempos: Vec<Duration> = Vec::new();
    for i in 0..edicoes {
        for k in 0..n_notas {
            editar(&mut sorteios[k], &mut atuais[k], 50);
            let mut t = atuais[k].clone();
            if matches!(fluxos, Fluxos::Copias) {
                t.push_str(&format!("\n[cópia {k}]"));
            }
            let ini = Instant::now();
            h.gravar(
                &NotaId::novo(format!("n{k}")),
                &t,
                em(1_000_000 + i as u64 * 60),
                Origem::Editor,
            )
            .unwrap();
            let dt = ini.elapsed();
            soma += dt;
            tempos.push(dt);
            cnt += 1;
            pior = pior.max(dt);
        }
    }
    tempos.sort();
    h.persistir().unwrap();
    let est = h.estatisticas().unwrap();

    // Abrir cada versão da nota 0. Três leituras de cada; o tempo da versão é o mínimo das três
    // (tira o ruído do sistema); `abrir_pior_bruto` é o maior tempo visto em qualquer leitura.
    let id = NotaId::novo("n0");
    let infos = h.versoes(&id).unwrap();
    let (mut soma_a, mut pior_a, mut pior_bruto) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    for v in &infos {
        let mut melhor = Duration::MAX;
        for _ in 0..3 {
            let ini = Instant::now();
            let t = h.ler(&id, v.id).unwrap();
            let dt = ini.elapsed();
            assert_eq!(t.len(), v.tamanho);
            melhor = melhor.min(dt);
            pior_bruto = pior_bruto.max(dt);
        }
        soma_a += melhor;
        pior_a = pior_a.max(melhor);
    }
    let sem = h.tamanho_em_disco().unwrap();
    let rel = h.compactar().unwrap();
    Resultado {
        arquivo_sem_compactar: sem,
        arquivo: rel.bytes_depois,
        registros: est.bytes_registros,
        bytes_pedacos: est.bytes_pedacos,
        pedacos: est.pedacos,
        gravar_medio: soma / cnt,
        gravar_pior: pior,
        gravar_p50: tempos[tempos.len() / 2],
        gravar_p99: tempos[tempos.len() * 99 / 100],
        abrir_medio: soma_a / infos.len() as u32,
        abrir_pior: pior_a,
        abrir_pior_bruto: pior_bruto,
    }
}

fn mostrar(nome: &str, r: &Resultado) {
    println!(
        "{nome}\n    arquivo {} B ({:.1} KB; {} B antes de compactar) | registros {} B | pedaços {} B em {} | \
         gravar médio {:?} (p50 {:?}, p99 {:?}, pior {:?}) | abrir médio {:?}, pior {:?} (pior leitura bruta {:?})",
        r.arquivo,
        r.arquivo as f64 / 1000.0,
        r.arquivo_sem_compactar,
        r.registros,
        r.bytes_pedacos,
        r.pedacos,
        r.gravar_medio,
        r.gravar_p50,
        r.gravar_p99,
        r.gravar_pior,
        r.abrir_medio,
        r.abrir_pior,
        r.abrir_pior_bruto
    );
}

#[test]
#[ignore]
fn medicao() {
    println!("=== 1 nota de 5 KB, 1.000 edições de ~50 caracteres");
    let a = rodar(Durabilidade::Adiada, 1, 1000, Fluxos::Independentes);
    mostrar("durabilidade adiada (padrão)", &a);
    let b = rodar(Durabilidade::Imediata, 1, 1000, Fluxos::Independentes);
    mostrar("durabilidade imediata (fsync a cada versão)", &b);

    if std::env::var("SO1").is_ok() {
        return;
    }
    println!("=== 10 notas, 1.000 edições cada");
    let c = rodar(Durabilidade::Adiada, 10, 1000, Fluxos::Independentes);
    mostrar("10 notas com edições diferentes (mesmo texto inicial)", &c);
    let d = rodar(Durabilidade::Adiada, 10, 1000, Fluxos::Copias);
    mostrar("10 cópias parecidas (mesmo fluxo, uma marca por nota)", &d);
    println!(
        "10 x (1 nota sozinha) = {} B de arquivo; 10 notas independentes juntas = {} B; 10 cópias = {} B",
        10 * a.arquivo,
        c.arquivo,
        d.arquivo
    );
    println!(
        "pedaços: 1 nota = {}; 10 independentes = {}; 10 cópias = {}",
        a.pedacos, c.pedacos, d.pedacos
    );
}
