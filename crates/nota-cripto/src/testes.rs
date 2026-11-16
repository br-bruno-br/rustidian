use super::*;
use proptest::prelude::*;

fn chave_fixa() -> ChaveCofre {
    ChaveCofre([7u8; 32])
}

fn hex_de(b: &[u8]) -> String {
    hex(b)
}

fn embrulhar_rapido(k: &ChaveCofre, frase: &str) -> Vec<u8> {
    k.embrulhar_com(frase, ParamsKdf::TESTE, sal_aleatorio(), nonce_aleatorio())
        .unwrap()
}

#[test]
fn ida_e_volta_varios_tamanhos() {
    let k = ChaveCofre::nova();
    let casos: Vec<Vec<u8>> = vec![
        vec![],
        vec![42],
        "Olá, ação à moda: ç ã é ñ 日本語 😀🦀".as_bytes().to_vec(),
        (0..10 * 1024 * 1024).map(|i| (i % 251) as u8).collect(),
    ];
    for c in casos {
        let id = k.id_de("pasta/nota.md").unwrap();
        let blob = k.cifrar("pasta/nota.md", &c).unwrap();
        let (caminho, conteudo) = k.decifrar(&id, &blob).unwrap();
        assert_eq!(caminho, "pasta/nota.md");
        assert_eq!(conteudo, c);
    }
}

#[test]
fn um_bit_trocado_em_cada_regiao_falha() {
    let k = chave_fixa();
    let id = k.id_de("a/b.md").unwrap();
    let blob = k.cifrar("a/b.md", "conteúdo secreto".as_bytes()).unwrap();
    let n = blob.len();
    let regioes = [
        ("versão", 0..1),
        ("nonce", 1..1 + TAM_NONCE),
        ("cifra", 1 + TAM_NONCE..n - TAM_TAG),
        ("tag", n - TAM_TAG..n),
    ];
    for (nome, r) in regioes {
        for i in r {
            for bit in [0u8, 7] {
                let mut ruim = blob.clone();
                ruim[i] ^= 1 << bit;
                let e = k.decifrar(&id, &ruim).unwrap_err();
                if i == 0 {
                    assert!(matches!(e, Erro::FormatoDesconhecido(_)), "{nome}");
                } else {
                    assert_eq!(e, Erro::Adulterado, "{nome} byte {i}");
                }
            }
        }
    }
}

#[test]
fn blob_truncado_ou_estendido_falha_sem_panic() {
    let k = chave_fixa();
    let id = k.id_de("a.md").unwrap();
    let blob = k.cifrar("a.md", b"x").unwrap();
    for n in 0..blob.len() {
        assert!(k.decifrar(&id, &blob[..n]).is_err());
    }
    let mut mais = blob.clone();
    mais.push(0);
    assert_eq!(k.decifrar(&id, &mais), Err(Erro::Adulterado));
}

#[test]
fn blob_trocado_de_id_da_adulterado() {
    let k = chave_fixa();
    let id_a = k.id_de("a.md").unwrap();
    let id_b = k.id_de("b.md").unwrap();
    let blob_a = k.cifrar("a.md", b"conteudo A").unwrap();
    assert_eq!(k.decifrar(&id_b, &blob_a), Err(Erro::Adulterado));
    assert!(k.decifrar(&id_a, &blob_a).is_ok());
}

#[test]
fn chave_errada_da_adulterado() {
    let k1 = ChaveCofre::nova();
    let k2 = ChaveCofre::nova();
    let blob = k1.cifrar("a.md", b"oi").unwrap();
    let id = k1.id_de("a.md").unwrap();
    assert_eq!(k2.decifrar(&id, &blob), Err(Erro::Adulterado));
}

#[test]
fn frase_errada_e_certa() {
    let k = ChaveCofre::nova();
    let cofre = embrulhar_rapido(&k, "frase correta");
    assert_eq!(
        ChaveCofre::desembrulhar(&cofre, "frase errada").unwrap_err(),
        Erro::FraseErrada
    );
    let k2 = ChaveCofre::desembrulhar(&cofre, "frase correta").unwrap();
    assert_eq!(*k.bytes(), *k2.bytes());
}

#[test]
fn frase_nfc_equivalente_abre() {
    let k = ChaveCofre::nova();
    let cofre = embrulhar_rapido(&k, "caf\u{e9} \u{e7}a");
    assert!(ChaveCofre::desembrulhar(&cofre, "cafe\u{301} c\u{327}a").is_ok());
}

#[test]
fn cofre_chave_truncado_ou_adulterado_nao_da_panic() {
    let k = ChaveCofre::nova();
    let cofre = embrulhar_rapido(&k, "f");
    for n in 0..cofre.len() {
        assert!(ChaveCofre::desembrulhar(&cofre[..n], "f").is_err(), "n={n}");
    }
    let mut mais = cofre.clone();
    mais.push(0);
    assert!(ChaveCofre::desembrulhar(&mais, "f").is_err());
    // Versão desconhecida.
    let mut v = cofre.clone();
    v[0] = 9;
    assert_eq!(
        ChaveCofre::desembrulhar(&v, "f").unwrap_err(),
        Erro::FormatoDesconhecido(9)
    );
    // Parâmetro trocado (t): a AAD pega, ou o limite recusa.
    let mut p = cofre.clone();
    p[5] ^= 1;
    assert!(ChaveCofre::desembrulhar(&p, "f").is_err());
    // Memória absurda é recusada antes de alocar.
    let mut grande = cofre.clone();
    grande[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        ChaveCofre::desembrulhar(&grande, "f").unwrap_err(),
        Erro::Adulterado
    );
    // Byte trocado no sal, no nonce e na cifra.
    for i in [14, 30, 60, cofre.len() - 1] {
        let mut x = cofre.clone();
        x[i] ^= 1;
        assert!(ChaveCofre::desembrulhar(&x, "f").is_err());
    }
}

#[test]
fn embrulhar_padrao_grava_parametros_do_plano() {
    // Custa ~1 s: confere só o cabeçalho gravado pelo caminho de produção.
    let k = chave_fixa();
    let cofre = k.embrulhar("f").unwrap();
    assert_eq!(cofre.len(), TAM_COFRE);
    assert_eq!(cofre[0], 1);
    assert_eq!(ler_u32(&cofre[1..5]), 64 * 1024);
    assert_eq!(ler_u32(&cofre[5..9]), 3);
    assert_eq!(ler_u32(&cofre[9..13]), 1);
    let k2 = ChaveCofre::desembrulhar(&cofre, "f").unwrap();
    assert_eq!(*k.bytes(), *k2.bytes());
}

#[test]
fn id_e_deterministico_e_normalizado() {
    let k = chave_fixa();
    let a = k.id_de("a\\b.md").unwrap();
    assert_eq!(a, k.id_de("./a/b.md").unwrap());
    assert_eq!(a, k.id_de("a/b.md").unwrap());
    assert_eq!(a, k.id_de("a//b.md").unwrap());
    assert_eq!(a.0.len(), 64);
    assert!(
        a.0.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
    assert_ne!(a, k.id_de("a/c.md").unwrap());
    assert_ne!(a, ChaveCofre([8u8; 32]).id_de("a/b.md").unwrap());
}

#[test]
fn nfc_composto_e_decomposto_dao_o_mesmo_id() {
    let k = chave_fixa();
    assert_eq!(
        k.id_de("notas/\u{e9}.md").unwrap(),
        k.id_de("notas/e\u{301}.md").unwrap()
    );
    assert_eq!(normalizar("e\u{301}.md").unwrap(), "\u{e9}.md");
}

#[test]
fn caminhos_invalidos_sao_recusados() {
    let k = chave_fixa();
    for ruim in [
        "",
        ".",
        "./",
        "/",
        "..",
        "../a.md",
        "a/../b.md",
        "a\\..\\b.md",
        "/abs",
        "\\abs",
        "C:\\x.md",
        "c:/x.md",
        "//srv/x",
        "a\0b",
    ] {
        assert!(
            matches!(normalizar(ruim), Err(Erro::CaminhoInvalido(_))),
            "{ruim:?}"
        );
        assert!(k.id_de(ruim).is_err());
        assert!(k.cifrar(ruim, b"x").is_err());
    }
    let longo = "a".repeat(CAMINHO_MAXIMO + 1);
    assert!(normalizar(&longo).is_err());
    // Nome que só parece ".." continua válido.
    assert_eq!(normalizar("..a/b..").unwrap(), "..a/b..");
}

#[test]
fn dois_cifrar_do_mesmo_conteudo_diferem() {
    let k = chave_fixa();
    let a = k.cifrar("a.md", b"igual").unwrap();
    let b = k.cifrar("a.md", b"igual").unwrap();
    assert_ne!(a, b);
    let id = k.id_de("a.md").unwrap();
    assert_eq!(k.decifrar(&id, &a).unwrap(), k.decifrar(&id, &b).unwrap());
}

#[test]
fn tamanho_excedido() {
    let k = chave_fixa();
    let grande = vec![0u8; TAMANHO_MAXIMO + 1];
    assert_eq!(k.cifrar("a.md", &grande), Err(Erro::TamanhoExcedido));
    let id = k.id_de("a.md").unwrap();
    // Blob gigante é recusado sem tentar decifrar.
    let enorme = vec![1u8; TAM_BLOB_MAXIMO + 1];
    assert_eq!(k.decifrar(&id, &enorme), Err(Erro::TamanhoExcedido));
}

#[test]
fn debug_nao_mostra_bytes_e_de_bytes_valida_tamanho() {
    let k = ChaveCofre([0xAB; 32]);
    let d = format!("{k:?}");
    assert!(!d.contains("171") && !d.to_lowercase().contains("ab"));
    assert!(ChaveCofre::de_bytes(&[0u8; 31]).is_err());
    assert!(ChaveCofre::de_bytes(&[0u8; 33]).is_err());
    assert_eq!(
        *ChaveCofre::de_bytes(&[5u8; 32]).unwrap().bytes(),
        [5u8; 32]
    );
}

const ID_ESPERADO: &str = "81ec912f0880c0de04b2e959822046ffa8cdfe84343ec570b18b5ac60dd13714";
const BLOB_ESPERADO: &str = "01090909090909090909090909090909090909090909090909cf9e877b18efa2f618bfd2659b05a8fb86841b52e3af08ea0aa80ed0a82ca837eef98ade1b3179d96c47e2bc19c771857595";
const COFRE_ESPERADO: &str = "011000000002000000010000000303030303030303030303030303030304040404040404040404040404040404040404040404040428cc9d07e6db87f56a52aa70e580186133cb545f185590a7f0eff2644925706c979e0e0a3948342cf1f9859024deaa86";

/// Trava o formato: chave, nonce, sal e caminho fixos dão sempre os mesmos bytes.
#[test]
fn vetor_fixo_trava_o_formato() {
    let k = ChaveCofre(core::array::from_fn(|i| i as u8));
    let id = k.id_de("Notas\\./diário/é.md").unwrap();
    let blob = k
        .cifrar_com_nonce("Notas/diário/é.md", "olá, mundo".as_bytes(), [9u8; 24])
        .unwrap();
    let cofre = k
        .embrulhar_com(
            "frase de teste",
            ParamsKdf {
                m_kib: 16,
                t: 2,
                p: 1,
            },
            [3u8; 16],
            [4u8; 24],
        )
        .unwrap();
    assert_eq!(id.0, ID_ESPERADO);
    assert_eq!(hex_de(&blob), BLOB_ESPERADO);
    assert_eq!(hex_de(&cofre), COFRE_ESPERADO);
    // E o vetor abre de volta.
    let (c, t) = k.decifrar(&id, &blob).unwrap();
    assert_eq!(
        (c.as_str(), t.as_slice()),
        ("Notas/diário/é.md", "olá, mundo".as_bytes())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn propriedade_ida_e_volta(
        conteudo in proptest::collection::vec(any::<u8>(), 0..2048),
        caminho in "[a-zA-Z0-9 é]{1,8}(/[a-zA-Z0-9 é]{1,8}){0,3}\\.md",
    ) {
        let k = ChaveCofre::nova();
        let id = k.id_de(&caminho).unwrap();
        let blob = k.cifrar(&caminho, &conteudo).unwrap();
        let (c, t) = k.decifrar(&id, &blob).unwrap();
        prop_assert_eq!(c, normalizar(&caminho).unwrap());
        prop_assert_eq!(t, conteudo);
        // Formas equivalentes do caminho dão o mesmo id.
        prop_assert_eq!(&id, &k.id_de(&format!("./{}", caminho.replace('/', "\\"))).unwrap());
    }

    #[test]
    fn propriedade_byte_trocado_sempre_falha(
        conteudo in proptest::collection::vec(any::<u8>(), 0..256),
        pos in any::<prop::sample::Index>(),
        xor in 1u8..=255,
    ) {
        let k = ChaveCofre::nova();
        let id = k.id_de("n.md").unwrap();
        let mut blob = k.cifrar("n.md", &conteudo).unwrap();
        let i = pos.index(blob.len());
        blob[i] ^= xor;
        prop_assert!(k.decifrar(&id, &blob).is_err());
    }

    #[test]
    fn propriedade_decifrar_lixo_nao_da_panic(lixo in proptest::collection::vec(any::<u8>(), 0..300)) {
        let k = chave_fixa();
        let id = k.id_de("n.md").unwrap();
        prop_assert!(k.decifrar(&id, &lixo).is_err());
    }
}
