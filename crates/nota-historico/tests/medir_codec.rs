//! Medição pontual do modo "com base" do zstd (ref_prefix x with_dictionary). Roda com
//! `cargo test --release -p nota-historico --test medir_codec -- --ignored --nocapture`.
mod gerador;
use gerador::*;
use nota_historico::{Codec, ZstdCodec};
use std::time::Instant;

#[test]
#[ignore]
fn medir_codec() {
    let mut s = Sorteio::novo(7);
    let mut t = texto(&mut s, 5000);
    let mut versoes = vec![t.clone()];
    for _ in 0..200 {
        editar(&mut s, &mut t, 50);
        versoes.push(t.clone());
    }
    println!("tamanho da nota: {} bytes", versoes[100].len());
    for nivel in [1, 3, 9, 19] {
        let c = ZstdCodec::novo(nivel).unwrap();
        let (mut solto, mut base_tam, mut dic) = (0usize, 0usize, 0usize);
        let t0 = Instant::now();
        for w in versoes.windows(2) {
            solto += c.comprimir(w[0].as_bytes()).unwrap().len();
        }
        let ts = t0.elapsed();
        let t0 = Instant::now();
        for w in versoes.windows(2) {
            let d = c
                .comprimir_com_base(w[0].as_bytes(), w[1].as_bytes())
                .unwrap();
            base_tam += d.len();
        }
        let tb = t0.elapsed();
        for w in versoes.windows(2) {
            let d = c
                .comprimir_com_base(w[0].as_bytes(), w[1].as_bytes())
                .unwrap();
            assert_eq!(
                c.descomprimir_com_base(&d, w[1].as_bytes(), w[0].len())
                    .unwrap(),
                w[0].as_bytes()
            );
        }
        let t0 = Instant::now();
        for w in versoes.windows(2) {
            let mut cc = zstd::bulk::Compressor::with_dictionary(nivel, w[1].as_bytes()).unwrap();
            dic += cc.compress(w[0].as_bytes()).unwrap().len();
        }
        let td = t0.elapsed();
        let n = versoes.len() as u32 - 1;
        println!(
            "nivel {nivel}: solto {} B ({:?}/un) | ref_prefix {} B ({:?}/un) | with_dictionary {} B ({:?}/un)",
            solto / n as usize,
            ts / n,
            base_tam / n as usize,
            tb / n,
            dic / n as usize,
            td / n
        );
    }
}

#[test]
#[ignore]
fn medir_reuso() {
    use zstd::zstd_safe::{CCtx, CParameter, ResetDirective};
    let mut s = Sorteio::novo(7);
    let mut t = texto(&mut s, 5000);
    let mut versoes = vec![t.clone()];
    for _ in 0..500 {
        editar(&mut s, &mut t, 50);
        versoes.push(t.clone());
    }
    let n = versoes.len() as u32 - 1;
    for nivel in [1, 3] {
        // A: contexto novo a cada chamada
        let c = ZstdCodec::novo(nivel).unwrap();
        let t0 = Instant::now();
        let mut tam = 0;
        for w in versoes.windows(2) {
            tam += c
                .comprimir_com_base(w[0].as_bytes(), w[1].as_bytes())
                .unwrap()
                .len();
        }
        println!(
            "nivel {nivel} A novo: {:?}/un ({} B)",
            t0.elapsed() / n,
            tam / n as usize
        );
        // B: CCtx reaproveitado, ref_prefix
        let mut cctx = CCtx::create();
        cctx.set_parameter(CParameter::CompressionLevel(nivel))
            .unwrap();
        let t0 = Instant::now();
        let mut tam = 0;
        for w in versoes.windows(2) {
            cctx.reset(ResetDirective::SessionOnly).unwrap();
            cctx.ref_prefix(w[1].as_bytes()).unwrap();
            let mut out: Vec<u8> = Vec::with_capacity(zstd::zstd_safe::compress_bound(w[0].len()));
            cctx.compress2(&mut out, w[0].as_bytes()).unwrap();
            tam += out.len();
        }
        println!(
            "nivel {nivel} B reuso+prefix: {:?}/un ({} B)",
            t0.elapsed() / n,
            tam / n as usize
        );
        // B2: Compressor reaproveitado com set_dictionary
        let mut comp = zstd::bulk::Compressor::new(nivel).unwrap();
        let t0 = Instant::now();
        let mut tam = 0;
        for w in versoes.windows(2) {
            comp.set_dictionary(nivel, w[1].as_bytes()).unwrap();
            tam += comp.compress(w[0].as_bytes()).unwrap().len();
        }
        println!(
            "nivel {nivel} B2 reuso+set_dictionary: {:?}/un ({} B)",
            t0.elapsed() / n,
            tam / n as usize
        );
    }
}
