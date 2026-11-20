//! Testes da H3 (árvore de desfazer) e da H4 (achar o que apaguei, histórico do trecho) contra um
//! banco de verdade, incluindo a regra do "apagar do histórico" e a migração de um cofre da H2.
mod gerador;

use gerador::{Sorteio, editar, texto};
use nota_historico::{
    Historico, LimitesDesfazer, NoId, NotaId, Origem, PoliticaRetencao, Registrado, VersaoId,
    historico_do_trecho,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn em(segundos: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(segundos)
}

fn nota(n: &str) -> NotaId {
    NotaId::novo(n)
}

fn novo() -> (tempfile::TempDir, std::path::PathBuf, Historico) {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let h = Historico::abrir(&caminho).unwrap();
    (d, caminho, h)
}

/// A -> B -> C, depois desfaz até B e digita D: a árvore tem dois galhos (C e D).
fn arvore_com_dois_galhos(h: &mut Historico, id: &NotaId) -> (NoId, NoId, NoId, NoId) {
    let a = h.desfazer_abrir(id, "A", em(1), 100).unwrap();
    assert!(!a.aplicada, "sem árvore antes: começa nova");
    let Registrado::Novo(b) = h.desfazer_registrar(id, "AB", em(2)).unwrap() else {
        panic!("B devia ser passo novo");
    };
    let Registrado::Novo(c) = h.desfazer_registrar(id, "ABC", em(3)).unwrap() else {
        panic!("C devia ser passo novo");
    };
    // Ctrl+Z: o texto volta a ser o de B.
    assert_eq!(
        h.desfazer_registrar(id, "AB", em(4)).unwrap(),
        Registrado::Movido(b)
    );
    let Registrado::Novo(d) = h.desfazer_registrar(id, "ABD", em(5)).unwrap() else {
        panic!("D devia ser passo novo");
    };
    (a.atual, b, c, d)
}

#[test]
fn arvore_nasce_registra_e_reconstroi_cada_passo() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    let a = h.desfazer_abrir(&id, "um", em(1), 100).unwrap();
    assert!(!a.aplicada && a.ancestrais.is_empty() && a.passos == 1);
    // Texto igual ao atual não faz nada.
    assert_eq!(
        h.desfazer_registrar(&id, "um", em(2)).unwrap(),
        Registrado::Nada
    );
    let mut passos = vec![a.atual];
    let textos = ["um dois", "um dois três", "um dois três quatro."];
    for (i, t) in textos.iter().enumerate() {
        match h.desfazer_registrar(&id, t, em(10 + i as u64)).unwrap() {
            Registrado::Novo(no) => passos.push(no),
            outro => panic!("{outro:?}"),
        }
    }
    let todos = ["um", "um dois", "um dois três", "um dois três quatro."];
    for (no, t) in passos.iter().zip(todos) {
        assert_eq!(h.desfazer_texto(&id, *no).unwrap(), t);
    }
    let g = h.desfazer_galhos(&id).unwrap();
    assert_eq!(g.passos, 4);
    assert_eq!(g.galhos.len(), 1, "um caminho só: um galho");
    assert!(g.galhos[0].atual);
    assert_eq!(g.galhos[0].passos, 4);
    assert_eq!(g.galhos[0].trecho, "quatro.");
    assert!(g.arquivadas.is_empty());
}

#[test]
fn desfazer_e_digitar_outra_coisa_cria_um_galho_e_o_caminho_antigo_fica() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    let (_a, b, c, d) = arvore_com_dois_galhos(&mut h, &id);
    let g = h.desfazer_galhos(&id).unwrap();
    assert_eq!(g.galhos.len(), 2);
    assert_eq!(g.galhos[0].no, d);
    assert!(g.galhos[0].atual);
    assert_eq!(g.galhos[1].no, c);
    assert!(
        !g.galhos[1].atual && !g.galhos[1].refazer,
        "C não está abaixo de D"
    );
    assert_eq!(h.desfazer_texto(&id, c).unwrap(), "ABC");
    assert_eq!(h.desfazer_texto(&id, d).unwrap(), "ABD");
    // Ir para o galho antigo: devolve o texto e a semente do Ctrl+Z (A, B).
    let (texto, ab) = h.desfazer_ir_para(&id, c, 100).unwrap();
    assert_eq!(texto, "ABC");
    assert_eq!(ab.ancestrais, ["A", "AB"]);
    assert_eq!(ab.atual, c);
    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.galhos.iter().find(|x| x.no == c).unwrap().atual);
    // E voltar: desfazer até B deixa C e D abaixo do atual ("refazer").
    assert_eq!(
        h.desfazer_registrar(&id, "AB", em(9)).unwrap(),
        Registrado::Movido(b)
    );
    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.galhos.iter().all(|x| x.refazer || x.atual));
    assert_eq!(g.galhos.len(), 3, "C, D e o passo atual (B)");
}

#[test]
fn reabrir_o_arquivo_devolve_o_caminho_para_o_ctrl_z() {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let id = nota("n");
    let ultimo = {
        let mut h = Historico::abrir(&caminho).unwrap();
        h.desfazer_abrir(&id, "v0", em(1), 100).unwrap();
        for i in 1..=5 {
            h.desfazer_registrar(&id, &format!("v0 {}", "x".repeat(i)), em(1 + i as u64))
                .unwrap();
        }
        h.persistir().unwrap();
        "v0 xxxxx".to_owned()
    };
    let mut h = Historico::abrir(&caminho).unwrap();
    let a = h.desfazer_abrir(&id, &ultimo, em(100), 100).unwrap();
    assert!(a.aplicada, "o texto é o do passo atual: a árvore vale");
    assert_eq!(
        a.ancestrais,
        ["v0", "v0 x", "v0 xx", "v0 xxx", "v0 xxxx"],
        "o Ctrl+Z da sessão anterior volta passo a passo"
    );
    // Poucos ancestrais: só os mais recentes.
    let a = h.desfazer_abrir(&id, &ultimo, em(101), 2).unwrap();
    assert_eq!(a.ancestrais, ["v0 xxx", "v0 xxxx"]);
}

#[test]
fn arquivo_que_voltou_a_um_passo_antigo_ainda_aplica_a_arvore() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    h.desfazer_abrir(&id, "A", em(1), 100).unwrap();
    h.desfazer_registrar(&id, "AB", em(2)).unwrap();
    h.desfazer_registrar(&id, "ABC", em(3)).unwrap();
    // O arquivo no disco tem o texto de B (por exemplo, o salvamento ficou um passo atrás).
    let a = h.desfazer_abrir(&id, "AB", em(10), 100).unwrap();
    assert!(a.aplicada);
    assert_eq!(a.ancestrais, ["A"]);
}

#[test]
fn arquivo_mudado_por_fora_arquiva_a_arvore_e_nunca_aplica_passos_no_texto_errado() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    h.desfazer_abrir(&id, "texto original", em(1), 100).unwrap();
    h.desfazer_registrar(&id, "texto original editado", em(2))
        .unwrap();
    let a = h
        .desfazer_abrir(&id, "texto vindo de outro aparelho", em(3), 100)
        .unwrap();
    assert!(!a.aplicada);
    assert!(a.ancestrais.is_empty(), "nenhum passo antigo se aplica");
    assert_eq!(a.passos, 1);
    let g = h.desfazer_galhos(&id).unwrap();
    assert_eq!(g.passos, 1);
    assert_eq!(
        g.arquivadas.len(),
        1,
        "a árvore antiga fica só como histórico"
    );
    assert_eq!(g.arquivadas[0].passos, 2);
    // A árvore nova segue normalmente.
    assert!(matches!(
        h.desfazer_registrar(&id, "texto vindo de outro aparelho!", em(4))
            .unwrap(),
        Registrado::Novo(_)
    ));
    // Só as duas arquivadas mais recentes ficam.
    for i in 0..4 {
        h.desfazer_abrir(&id, &format!("mudou fora {i}"), em(10 + i), 100)
            .unwrap();
    }
    assert_eq!(h.desfazer_galhos(&id).unwrap().arquivadas.len(), 2);
}

#[test]
fn limite_descarta_os_mais_antigos_e_tudo_que_fica_reconstroi() {
    let (_d, _c, mut h) = novo();
    h.ajustar_limites_desfazer(LimitesDesfazer {
        max_passos: 40,
        max_bytes: 4 * 1024 * 1024,
    });
    let id = nota("n");
    let mut s = Sorteio::novo(7);
    let mut t = texto(&mut s, 800);
    h.desfazer_abrir(&id, &t, em(1), 100).unwrap();
    let mut ultimo_texto = t.clone();
    for i in 0..300u64 {
        editar(&mut s, &mut t, 30);
        h.desfazer_registrar(&id, &t, em(2 + i)).unwrap();
        ultimo_texto = t.clone();
    }
    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.passos <= 40, "{} passos", g.passos);
    assert!(g.passos >= 30, "a poda deixa 90% do limite, não esvazia");
    // O passo atual sobreviveu e reconstrói; cada galho também.
    let atual = g.galhos.iter().find(|x| x.atual).unwrap();
    assert_eq!(h.desfazer_texto(&id, atual.no).unwrap(), ultimo_texto);
    // E o Ctrl+Z reaberto ainda traz o caminho que sobrou.
    let a = h.desfazer_abrir(&id, &ultimo_texto, em(999), 100).unwrap();
    assert!(a.aplicada);
    assert!(a.ancestrais.len() >= 20, "{}", a.ancestrais.len());
}

#[test]
fn limite_de_bytes_tambem_corta() {
    let (_d, _c, mut h) = novo();
    h.ajustar_limites_desfazer(LimitesDesfazer {
        max_passos: 100_000,
        max_bytes: 16 * 1024,
    });
    let id = nota("n");
    let mut s = Sorteio::novo(3);
    let mut t = texto(&mut s, 3000);
    h.desfazer_abrir(&id, &t, em(1), 100).unwrap();
    for i in 0..200u64 {
        editar(&mut s, &mut t, 40);
        h.desfazer_registrar(&id, &t, em(2 + i)).unwrap();
    }
    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.passos < 200, "{} passos", g.passos);
    let atual = g.galhos.iter().find(|x| x.atual).unwrap();
    assert_eq!(h.desfazer_texto(&id, atual.no).unwrap(), t);
}

#[test]
fn a_arvore_sobrevive_a_podar_compactar_e_renomear() {
    let (_d, _c, mut h) = novo();
    let id = nota("antigo/nome");
    h.gravar(&id, "A", em(1), Origem::Editor).unwrap();
    let (_a, _b, c, d) = arvore_com_dois_galhos(&mut h, &id);
    h.podar(em(10_000_000), &PoliticaRetencao::default())
        .unwrap();
    h.compactar().unwrap();
    assert_eq!(h.desfazer_texto(&id, c).unwrap(), "ABC");
    assert!(h.renomear_nota(&id, &nota("novo/nome")).unwrap());
    let novo_id = nota("novo/nome");
    assert_eq!(h.desfazer_texto(&novo_id, d).unwrap(), "ABD");
    assert_eq!(h.desfazer_galhos(&novo_id).unwrap().galhos.len(), 2);
    assert_eq!(h.desfazer_galhos(&id).unwrap().galhos.len(), 0);
}

#[test]
fn banco_gravado_antes_da_h3_abre_ganha_as_tabelas_e_nao_perde_versoes() {
    use redb::TableDefinition;
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let id = nota("n");
    let mut s = Sorteio::novo(11);
    let mut t = texto(&mut s, 500);
    let mut textos = Vec::new();
    {
        let mut h = Historico::abrir(&caminho).unwrap();
        for i in 0..200u64 {
            editar(&mut s, &mut t, 30);
            h.gravar(&id, &t, em(100 + i), Origem::Editor).unwrap();
            textos.push(t.clone());
        }
        h.persistir().unwrap();
    }
    // Volta o arquivo ao formato da H2: sem as tabelas novas.
    {
        let db = redb::Database::create(&caminho).unwrap();
        let w = db.begin_write().unwrap();
        let arvores: TableDefinition<(u32, u32), &[u8]> = TableDefinition::new("arvores");
        let nos: TableDefinition<(u32, u32, u32), &[u8]> = TableDefinition::new("nos");
        assert!(w.delete_table(arvores).unwrap());
        assert!(w.delete_table(nos).unwrap());
        w.commit().unwrap();
    }
    let mut h = Historico::abrir(&caminho).unwrap();
    let infos = h.versoes(&id).unwrap();
    assert_eq!(infos.len(), 200);
    for (i, t) in textos.iter().enumerate() {
        assert_eq!(&h.ler(&id, VersaoId(i as u64 + 1)).unwrap(), t);
    }
    // E a árvore já funciona nele.
    let a = h.desfazer_abrir(&id, &t, em(5000), 100).unwrap();
    assert!(!a.aplicada);
    editar(&mut s, &mut t, 30);
    assert!(matches!(
        h.desfazer_registrar(&id, &t, em(5001)).unwrap(),
        Registrado::Novo(_)
    ));
    h.conferir_referencias().unwrap();
}

#[test]
fn apagar_do_historico_tira_o_trecho_da_arvore_ativa_e_das_arquivadas() {
    let (_d, caminho, mut h) = novo();
    let id = nota("n");
    let segredo = "senha-ultra-secreta-123";
    h.gravar(&id, "texto limpo", em(1), Origem::Editor).unwrap();
    h.desfazer_abrir(&id, "texto limpo", em(1), 100).unwrap();
    h.desfazer_registrar(&id, &format!("texto limpo {segredo}"), em(2))
        .unwrap();
    h.desfazer_registrar(&id, &format!("texto limpo {segredo} fim"), em(3))
        .unwrap();
    // Uma árvore arquivada que também tem o segredo.
    h.desfazer_abrir(&id, &format!("outro texto {segredo}"), em(4), 100)
        .unwrap();
    h.desfazer_registrar(&id, "outro texto", em(5)).unwrap();
    h.desfazer_registrar(&id, &format!("outro texto {segredo} de novo"), em(6))
        .unwrap();
    // Antes: o resumo do passo (texto cru no registro) e os passos têm o segredo.
    let (mut h, achou) = bruto_contem(h, &caminho, segredo);
    assert!(achou, "o resumo guarda o texto cru");
    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.galhos.iter().any(|x| x.trecho.contains(segredo)));

    h.apagar_texto_do_historico(&id, segredo).unwrap();

    let g = h.desfazer_galhos(&id).unwrap();
    assert!(g.galhos.iter().all(|x| !x.trecho.contains(segredo)));
    for galho in &g.galhos {
        let t = h.desfazer_texto(&id, galho.no).unwrap();
        assert!(!t.contains(segredo), "passo {:?}: {t}", galho.no);
    }
    assert_eq!(g.arquivadas.len(), 1, "a antiga continua só que limpa");
    let (mut h, achou) = bruto_contem(h, &caminho, segredo);
    assert!(!achou, "nem nos bytes do arquivo");
    // Nem nas versões, nem em "achar o que apaguei".
    for v in h.versoes(&id).unwrap() {
        assert!(!h.ler(&id, v.id).unwrap().contains(segredo));
    }
    // A árvore ativa continua coerente e aceita passos novos.
    let atual = g.galhos.iter().find(|x| x.atual).unwrap();
    let texto_atual = h.desfazer_texto(&id, atual.no).unwrap();
    assert_eq!(
        h.desfazer_registrar(&id, &texto_atual, em(7)).unwrap(),
        Registrado::Nada
    );
    assert!(matches!(
        h.desfazer_registrar(&id, "texto novo depois", em(8))
            .unwrap(),
        Registrado::Novo(_)
    ));
}

#[test]
fn apagar_de_todas_as_notas_tambem_limpa_as_arvores() {
    let (_d, caminho, mut h) = novo();
    let segredo = "cartao-4111-1111";
    for n in ["a", "b"] {
        let id = nota(n);
        h.gravar(&id, "inicio", em(1), Origem::Editor).unwrap();
        h.desfazer_abrir(&id, "inicio", em(1), 100).unwrap();
        h.desfazer_registrar(&id, &format!("inicio {segredo}"), em(2))
            .unwrap();
    }
    let (mut h, achou) = bruto_contem(h, &caminho, segredo);
    assert!(achou);
    let r = h.apagar_texto_de_todas(segredo).unwrap();
    assert_eq!(r.notas, 2);
    let (h, achou) = bruto_contem(h, &caminho, segredo);
    assert!(!achou);
    for n in ["a", "b"] {
        let g = h.desfazer_galhos(&nota(n)).unwrap();
        for galho in g.galhos {
            assert!(
                !h.desfazer_texto(&nota(n), galho.no)
                    .unwrap()
                    .contains(segredo)
            );
        }
    }
}

/// Grava, fecha, lê os bytes brutos do arquivo e reabre (no Windows o arquivo aberto fica travado).
fn bruto_contem(h: Historico, caminho: &std::path::Path, trecho: &str) -> (Historico, bool) {
    let mut h = h;
    h.persistir().unwrap();
    drop(h);
    let b = std::fs::read(caminho).unwrap();
    let achou = b.windows(trecho.len()).any(|j| j == trecho.as_bytes());
    (Historico::abrir(caminho).unwrap(), achou)
}

#[test]
fn passeio_aleatorio_toda_ponta_reconstroi_com_o_hash_gravado() {
    for semente in 1..=6u64 {
        let d = tempfile::tempdir().unwrap();
        let caminho = d.path().join("h.redb");
        let id = nota("n");
        let mut s = Sorteio::novo(semente);
        let mut t = texto(&mut s, 600);
        let mut h = Historico::abrir(&caminho).unwrap();
        h.ajustar_limites_desfazer(LimitesDesfazer {
            max_passos: 60,
            max_bytes: 1024 * 1024,
        });
        h.desfazer_abrir(&id, &t, em(1), 100).unwrap();
        let mut historia: Vec<String> = vec![t.clone()];
        for passo in 0..250u64 {
            match s.ate(10) {
                0..=5 => {
                    editar(&mut s, &mut t, 25);
                    h.desfazer_registrar(&id, &t, em(2 + passo)).unwrap();
                    historia.push(t.clone());
                }
                6 | 7 => {
                    // Desfaz para um texto que já existiu.
                    let i = s.ate(historia.len());
                    t = historia[i].clone();
                    h.desfazer_registrar(&id, &t, em(2 + passo)).unwrap();
                }
                8 => {
                    let g = h.desfazer_galhos(&id).unwrap();
                    let um = &g.galhos[s.ate(g.galhos.len())];
                    let (texto_do_galho, ab) = h.desfazer_ir_para(&id, um.no, 100).unwrap();
                    assert_eq!(ab.atual, um.no);
                    t = texto_do_galho;
                }
                _ => {
                    // Fecha e reabre o app: o texto do arquivo é o atual.
                    h.persistir().unwrap();
                    drop(h);
                    h = Historico::abrir(&caminho).unwrap();
                    h.ajustar_limites_desfazer(LimitesDesfazer {
                        max_passos: 60,
                        max_bytes: 1024 * 1024,
                    });
                    let a = h.desfazer_abrir(&id, &t, em(2 + passo), 100).unwrap();
                    assert!(a.aplicada, "semente {semente}, passo {passo}");
                }
            }
        }
        let g = h.desfazer_galhos(&id).unwrap();
        assert!(g.passos <= 60);
        for galho in &g.galhos {
            // `desfazer_texto` confere o blake3 gravado de cada passo do caminho.
            let texto_do_galho = h.desfazer_texto(&id, galho.no).unwrap();
            assert!(!texto_do_galho.is_empty() || galho.passos > 0);
        }
        let atual = g.galhos.iter().find(|x| x.atual).unwrap();
        assert_eq!(
            h.desfazer_texto(&id, atual.no).unwrap(),
            t,
            "semente {semente}"
        );
    }
}

// ----------------------------------------------------------------------------------- H4

fn gravar_versoes(h: &mut Historico, id: &NotaId, textos: &[&str]) {
    for (i, t) in textos.iter().enumerate() {
        h.gravar(id, t, em(100 * (i as u64 + 1)), Origem::Editor)
            .unwrap();
    }
}

#[test]
fn achar_apagados_no_banco_com_data_e_busca() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    gravar_versoes(
        &mut h,
        &id,
        &[
            "# Diário\n\nA cafeteira antiga ficava na cozinha. Hoje choveu o dia todo.\n",
            "# Diário\n\nA cafeteira antiga ficava na cozinha. Hoje choveu o dia todo. O violão voltou do conserto.\n",
            "# Diário\n\nHoje choveu o dia todo. O violão voltou do conserto.\n",
        ],
    );
    let atual = "# Diário\n\nHoje choveu o dia todo. O violão voltou do conserto.\n";
    let a = h.achar_apagados(&id, atual).unwrap();
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].trecho, "A cafeteira antiga ficava na cozinha.");
    assert_eq!(a[0].existiu_de, em(100));
    assert_eq!(a[0].existiu_ate, em(200));
    assert_eq!(a[0].apagado_em, Some(em(300)));
    assert_eq!(a[0].versoes, 2);
    assert_eq!(
        nota_historico::filtrar_apagados(&a, "CAFETEIRA cozinha").len(),
        1
    );
    assert!(nota_historico::filtrar_apagados(&a, "violão").is_empty());
}

#[test]
fn achar_apagados_nao_traz_de_volta_o_que_foi_apagado_do_historico() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    let segredo = "a senha do banco é 9981-xx";
    gravar_versoes(
        &mut h,
        &id,
        &[
            &format!("Anotações de hoje. Lembrete: {segredo}. Comprar pão.\n"),
            "Anotações de hoje. Comprar pão.\n",
        ],
    );
    let atual = "Anotações de hoje. Comprar pão.\n";
    let antes = h.achar_apagados(&id, atual).unwrap();
    assert!(
        antes.iter().any(|a| a.trecho.contains("9981-xx")),
        "{antes:?}"
    );
    h.apagar_texto_do_historico(&id, segredo).unwrap();
    let depois = h.achar_apagados(&id, atual).unwrap();
    assert!(
        depois
            .iter()
            .all(|a| !a.trecho.contains("9981") && !a.trecho.contains("senha do banco")),
        "o trecho apagado do histórico não pode reaparecer: {depois:?}"
    );
    // Nem pelo histórico do trecho.
    let versoes = h.textos_da_nota(&id, 100).unwrap();
    assert!(versoes.iter().all(|(_, t)| !t.contains("9981")));
    let h_trecho = h.historico_do_trecho(&id, atual, 0..atual.len()).unwrap();
    assert!(h_trecho.iter().all(|m| !m.texto.contains("9981")));
}

#[test]
fn textos_em_lote_batem_com_ler_uma_a_uma() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    let mut s = Sorteio::novo(5);
    let mut t = texto(&mut s, 700);
    for i in 0..300u64 {
        editar(&mut s, &mut t, 30);
        h.gravar(&id, &t, em(i), Origem::Editor).unwrap();
    }
    let lote = h.textos_da_nota(&id, 10_000).unwrap();
    assert_eq!(lote.len(), 300);
    for (info, texto) in &lote {
        assert_eq!(&h.ler(&id, info.id).unwrap(), texto);
    }
    let recentes = h.textos_da_nota(&id, 50).unwrap();
    assert_eq!(recentes.len(), 50);
    assert_eq!(recentes.last().unwrap().1, lote.last().unwrap().1);
    assert_eq!(recentes[0].0.id, lote[250].0.id);
}

#[test]
fn historico_do_trecho_no_banco() {
    let (_d, _c, mut h) = novo();
    let id = nota("n");
    gravar_versoes(
        &mut h,
        &id,
        &[
            "Cena um.\n\nO velho entrou no bar.\n\nFim.\n",
            "Cena um.\n\nO velho cansado entrou no bar.\n\nFim.\n",
            "Cena um, revisada.\n\nO velho cansado entrou no bar.\n\nFim.\n",
        ],
    );
    let atual = "Cena um, revisada.\n\nO velho cansado entrou no bar.\n\nFim.\n";
    let ini = atual.find("O velho").unwrap();
    let r = h
        .historico_do_trecho(
            &id,
            atual,
            ini..ini + "O velho cansado entrou no bar.".len(),
        )
        .unwrap();
    let textos: Vec<&str> = r.iter().map(|m| m.texto.as_str()).collect();
    assert_eq!(
        textos,
        ["O velho cansado entrou no bar.", "O velho entrou no bar."]
    );
    // A função pura devolve o mesmo.
    let versoes = h.textos_da_nota(&id, 100).unwrap();
    assert_eq!(
        historico_do_trecho(
            &versoes,
            atual,
            ini..ini + "O velho cansado entrou no bar.".len()
        ),
        r
    );
}
