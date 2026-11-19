//! Testes do contrato do `Historico` (gravar, ler, reabrir, nomear, podar, compactar, apagar).
mod gerador;

use gerador::{Sorteio, editar, texto};
use nota_historico::{
    Codec, Durabilidade, Erro, Historico, NotaId, Opcoes, Origem, ParametrosPedacos,
    PoliticaRetencao, VersaoId, ZstdCodec,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DIA: u64 = 86_400;

fn em(segundos: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(segundos)
}

fn nota(n: &str) -> NotaId {
    NotaId::novo(n)
}

fn opcoes_pequenas(kf: u64) -> Opcoes {
    Opcoes {
        quadro_chave_a_cada: kf,
        durabilidade: Durabilidade::Adiada,
        pedacos: ParametrosPedacos {
            minimo: 32,
            bits_mascara: 5,
            maximo: 256,
        },
    }
}

/// Grava `n` versões encadeadas e devolve os textos (índice = versão - 1).
fn gravar_serie(h: &mut Historico, id: &NotaId, n: usize, semente: u64, t0: u64) -> Vec<String> {
    let mut s = Sorteio::novo(semente);
    let mut t = texto(&mut s, 1500);
    let mut textos = Vec::new();
    for i in 0..n {
        editar(&mut s, &mut t, 40);
        let v = h
            .gravar(id, &t, em(t0 + i as u64 * 60), Origem::Editor)
            .unwrap();
        assert_eq!(v, Some(VersaoId(i as u64 + 1)));
        textos.push(t.clone());
    }
    textos
}

#[test]
fn grava_e_le_todas_as_versoes() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    let textos = gravar_serie(&mut h, &id, 300, 1, 1_000_000);
    let infos = h.versoes(&id).unwrap();
    assert_eq!(infos.len(), 300);
    for (i, t) in textos.iter().enumerate() {
        assert_eq!(
            &h.ler(&id, VersaoId(i as u64 + 1)).unwrap(),
            t,
            "versão {}",
            i + 1
        );
        assert_eq!(infos[i].tamanho, t.len());
        assert_eq!(infos[i].impressao, *blake3::hash(t.as_bytes()).as_bytes());
        assert_eq!(infos[i].origem, Origem::Editor);
        assert_eq!(infos[i].quando, em(1_000_000 + i as u64 * 60));
    }
    h.conferir_referencias().unwrap();
}

#[test]
fn texto_identico_ao_ultimo_nao_grava() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    assert_eq!(
        h.gravar(&id, "um", em(10), Origem::Editor).unwrap(),
        Some(VersaoId(1))
    );
    assert_eq!(h.gravar(&id, "um", em(11), Origem::Editor).unwrap(), None);
    assert_eq!(
        h.gravar(&id, "dois", em(12), Origem::Externa).unwrap(),
        Some(VersaoId(2))
    );
    // Voltar ao texto de duas versões atrás é uma versão nova (só a última conta).
    assert_eq!(
        h.gravar(&id, "um", em(13), Origem::Restauracao).unwrap(),
        Some(VersaoId(3))
    );
    assert_eq!(h.versoes(&id).unwrap().len(), 3);
}

#[test]
fn texto_vazio_e_unicode_funcionam() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    let casos = [
        "",
        "ação 😀 coração",
        "",
        "só uma linha\n",
        "日本語のノート",
    ];
    for (i, t) in casos.iter().enumerate() {
        h.gravar(&id, t, em(i as u64), Origem::Sync).unwrap();
    }
    for (i, t) in casos.iter().enumerate() {
        assert_eq!(&h.ler(&id, VersaoId(i as u64 + 1)).unwrap(), t);
    }
}

#[test]
fn notas_desconhecidas_e_versoes_inexistentes() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    assert!(h.versoes(&nota("x")).unwrap().is_empty());
    assert!(matches!(
        h.ler(&nota("x"), VersaoId(1)),
        Err(Erro::NaoEncontrada(_))
    ));
    h.gravar(&nota("x"), "a", em(1), Origem::Editor).unwrap();
    assert!(matches!(
        h.ler(&nota("x"), VersaoId(9)),
        Err(Erro::NaoEncontrada(_))
    ));
    assert!(matches!(
        h.nomear(&nota("x"), VersaoId(9), Some("n".into())),
        Err(Erro::NaoEncontrada(_))
    ));
}

#[test]
fn reaberto_depois_de_fechar_mantem_tudo() {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let id = nota("a");
    let textos;
    {
        let mut h = Historico::abrir(&caminho).unwrap();
        textos = gravar_serie(&mut h, &id, 150, 2, 5_000);
        h.nomear(&id, VersaoId(7), Some("rascunho 2".into()))
            .unwrap();
        // Sem persistir() explícito: o Drop persiste.
    }
    let mut h = Historico::abrir(&caminho).unwrap();
    let infos = h.versoes(&id).unwrap();
    assert_eq!(infos.len(), 150);
    assert_eq!(infos[6].nome.as_deref(), Some("rascunho 2"));
    for (i, t) in textos.iter().enumerate() {
        assert_eq!(&h.ler(&id, VersaoId(i as u64 + 1)).unwrap(), t);
    }
    // E continua gravando a partir do número certo.
    assert_eq!(
        h.gravar(&id, "novo", em(9_999_999), Origem::Editor)
            .unwrap(),
        Some(VersaoId(151))
    );
    h.conferir_referencias().unwrap();
}

#[test]
fn durabilidade_imediata_tambem_reabre() {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let id = nota("a");
    let opc = Opcoes {
        durabilidade: Durabilidade::Imediata,
        ..Opcoes::default()
    };
    {
        let mut h = Historico::abrir_com(&caminho, opc, Box::new(ZstdCodec::default())).unwrap();
        gravar_serie(&mut h, &id, 20, 3, 100);
    }
    let h = Historico::abrir(&caminho).unwrap();
    assert_eq!(h.versoes(&id).unwrap().len(), 20);
}

#[test]
fn nomear_e_tirar_o_nome() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    gravar_serie(&mut h, &id, 5, 4, 100);
    h.nomear(&id, VersaoId(2), Some("enviado ao editor".into()))
        .unwrap();
    assert_eq!(
        h.versoes(&id).unwrap()[1].nome.as_deref(),
        Some("enviado ao editor")
    );
    h.nomear(&id, VersaoId(2), None).unwrap();
    assert_eq!(h.versoes(&id).unwrap()[1].nome, None);
    // O texto continua abrindo depois de mexer no nome.
    h.ler(&id, VersaoId(2)).unwrap();
}

#[test]
fn poda_segue_a_politica_e_tudo_que_fica_abre_igual() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir_com(
        &d.path().join("h.redb"),
        opcoes_pequenas(8),
        Box::new(ZstdCodec::default()),
    )
    .unwrap();
    let id = nota("a");
    let agora = 400 * DIA;
    // Uma versão a cada 20 minutos durante 3 dias, terminando há 60 dias; mais 20 recentes.
    let mut s = Sorteio::novo(5);
    let mut t = texto(&mut s, 800);
    let mut textos = std::collections::HashMap::new();
    let mut quando = Vec::new();
    for i in 0..216u64 {
        editar(&mut s, &mut t, 30);
        let q = agora - 63 * DIA + i * 1200;
        let v = h.gravar(&id, &t, em(q), Origem::Editor).unwrap().unwrap();
        textos.insert(v.0, t.clone());
        quando.push((v.0, q));
    }
    for i in 0..20u64 {
        editar(&mut s, &mut t, 30);
        let q = agora - 3600 + i * 60;
        let v = h.gravar(&id, &t, em(q), Origem::Editor).unwrap().unwrap();
        textos.insert(v.0, t.clone());
        quando.push((v.0, q));
    }
    h.nomear(&id, VersaoId(10), Some("marco".into())).unwrap();
    let antes = h.versoes(&id).unwrap().len();
    let rel = h.podar(em(agora), &PoliticaRetencao::default()).unwrap();
    let depois = h.versoes(&id).unwrap();
    assert_eq!(antes - depois.len(), rel.versoes_removidas);
    assert_eq!(rel.notas_afetadas, 1);
    // 63 dias atrás: 1 por semana (menos de 10 semanas) + a nomeada + as 20 recentes.
    assert!(depois.len() <= 20 + 1 + 10 + 1, "sobraram {}", depois.len());
    assert!(depois.iter().any(|v| v.nome.as_deref() == Some("marco")));
    assert_eq!(depois.last().unwrap().id, VersaoId(236));
    for v in &depois {
        assert_eq!(
            h.ler(&id, v.id).unwrap(),
            textos[&v.id.0],
            "versão {}",
            v.id.0
        );
    }
    h.conferir_referencias().unwrap();
    // Idempotente: podar de novo não tira nada.
    assert_eq!(
        h.podar(em(agora), &PoliticaRetencao::default())
            .unwrap()
            .versoes_removidas,
        0
    );
    // E ainda dá para gravar depois da poda.
    t.push_str(" fim.");
    h.gravar(&id, &t, em(agora + 5), Origem::Editor).unwrap();
    assert_eq!(h.ler(&id, VersaoId(237)).unwrap(), t);
}

#[test]
fn compactar_preserva_tudo_e_libera_espaco_depois_da_poda() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    let agora = 500 * DIA;
    let mut s = Sorteio::novo(6);
    let mut t = texto(&mut s, 5000);
    let mut textos = std::collections::HashMap::new();
    for i in 0..400u64 {
        editar(&mut s, &mut t, 50);
        let v = h
            .gravar(&id, &t, em(agora - 200 * DIA + i * 3600), Origem::Editor)
            .unwrap()
            .unwrap();
        textos.insert(v.0, t.clone());
    }
    let r_poda = h.podar(em(agora), &PoliticaRetencao::default()).unwrap();
    assert!(r_poda.versoes_removidas > 100);
    let tam_antes = h.tamanho_em_disco().unwrap();
    let rel = h.compactar().unwrap();
    assert!(rel.bytes_depois <= tam_antes, "{rel:?}");
    for v in h.versoes(&id).unwrap() {
        assert_eq!(h.ler(&id, v.id).unwrap(), textos[&v.id.0]);
    }
    h.conferir_referencias().unwrap();
}

#[test]
fn dedupe_entre_notas_guarda_os_pedacos_uma_vez() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let mut s = Sorteio::novo(8);
    let t = texto(&mut s, 20_000);
    h.gravar(&nota("a"), &t, em(1), Origem::Editor).unwrap();
    let um = h.estatisticas().unwrap();
    for i in 0..9 {
        h.gravar(&nota(&format!("copia{i}")), &t, em(2), Origem::Sync)
            .unwrap();
    }
    let dez = h.estatisticas().unwrap();
    assert_eq!(dez.notas, 10);
    assert_eq!(
        dez.pedacos, um.pedacos,
        "pedaços repetidos devem ser guardados uma vez"
    );
    assert_eq!(dez.bytes_pedacos, um.bytes_pedacos);
    h.conferir_referencias().unwrap();
}

#[test]
fn estatisticas_por_nota() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    let id = nota("a");
    gravar_serie(&mut h, &id, 100, 9, 100);
    let e = h.estatisticas_nota(&id).unwrap();
    assert_eq!(e.versoes, 100);
    assert!(e.bytes_texto > 100_000);
    assert!(e.bytes_guardados * 5 < e.bytes_texto, "{e:?}");
    assert_eq!(h.estatisticas_nota(&nota("zzz")).unwrap().versoes, 0);
    assert_eq!(h.notas().unwrap(), vec![id]);
}

#[test]
fn apagar_texto_some_de_todas_as_versoes_e_dos_bytes() {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let mut h =
        Historico::abrir_com(&caminho, opcoes_pequenas(8), Box::new(ZstdCodec::default())).unwrap();
    let id = nota("a");
    let outra = nota("b");
    let segredo = "SENHA-DO-BANCO-9f3a7c-xyz";
    let mut s = Sorteio::novo(10);
    let mut t = texto(&mut s, 1200);
    let mut modelo = Vec::new();
    for i in 0..60u64 {
        editar(&mut s, &mut t, 25);
        if i == 20 {
            let meio = t.char_indices().nth(400).unwrap().0;
            t.insert_str(meio, segredo);
        }
        if i == 40 {
            t = t.replace(segredo, "");
        }
        h.gravar(&id, &t, em(100 + i), Origem::Editor).unwrap();
        modelo.push(t.clone());
    }
    // O mesmo texto numa outra nota não pode ser tocado.
    h.gravar(
        &outra,
        &format!("nota b com {segredo} dentro"),
        em(5),
        Origem::Editor,
    )
    .unwrap();
    assert!(modelo.iter().any(|m| m.contains(segredo)));

    let n = h.apagar_texto_do_historico(&id, segredo).unwrap();
    assert_eq!(n, modelo.iter().filter(|m| m.contains(segredo)).count());

    for (i, m) in modelo.iter().enumerate() {
        let esperado = m.replace(segredo, "");
        let lido = h.ler(&id, VersaoId(i as u64 + 1)).unwrap();
        assert_eq!(lido, esperado, "versão {}", i + 1);
        assert!(!lido.contains(segredo));
    }
    // Nenhum pedaço guardado (descomprimido) contém o segredo, a não ser os da outra nota.
    let b = h.ler(&outra, VersaoId(1)).unwrap();
    assert!(b.contains(segredo));
    let com_segredo = h
        .pedacos_descomprimidos()
        .unwrap()
        .iter()
        .filter(|p| contem(p, segredo.as_bytes()))
        .count();
    assert_eq!(com_segredo, 1, "só o pedaço da nota b pode ter o segredo");
    h.conferir_referencias().unwrap();
    // Apagar na nota b limpa o resto.
    assert_eq!(h.apagar_texto_do_historico(&outra, segredo).unwrap(), 1);
    assert!(
        !h.pedacos_descomprimidos()
            .unwrap()
            .iter()
            .any(|p| contem(p, segredo.as_bytes()))
    );
    h.persistir().unwrap();
    drop(h);
    // E nos bytes brutos do arquivo (o zstd de blocos curtos às vezes guarda o texto cru).
    let bruto = std::fs::read(&caminho).unwrap();
    assert!(
        !contem(&bruto, segredo.as_bytes()),
        "segredo nos bytes do arquivo"
    );
    // Reabre e continua tudo certo.
    let h = Historico::abrir(&caminho).unwrap();
    assert!(!h.ler(&id, VersaoId(30)).unwrap().contains(segredo));
}

fn contem(palheiro: &[u8], agulha: &[u8]) -> bool {
    palheiro.windows(agulha.len()).any(|w| w == agulha)
}

#[test]
fn apagar_trecho_vazio_e_erro() {
    let d = tempfile::tempdir().unwrap();
    let mut h = Historico::abrir(&d.path().join("h.redb")).unwrap();
    h.gravar(&nota("a"), "x", em(1), Origem::Editor).unwrap();
    assert!(matches!(
        h.apagar_texto_do_historico(&nota("a"), ""),
        Err(Erro::Argumento(_))
    ));
    assert_eq!(
        h.apagar_texto_do_historico(&nota("a"), "inexistente")
            .unwrap(),
        0
    );
}

/// Codec que estraga a saída da descompressão sem mudar o tamanho: a conferência blake3 precisa pegar.
struct CodecRuim(ZstdCodec);

impl Codec for CodecRuim {
    fn nome(&self) -> &'static str {
        "zstd"
    }
    fn comprimir(&self, d: &[u8]) -> nota_historico::Result<Vec<u8>> {
        self.0.comprimir(d)
    }
    fn descomprimir(&self, c: &[u8], t: usize) -> nota_historico::Result<Vec<u8>> {
        let mut v = self.0.descomprimir(c, t)?;
        if let Some(b) = v.first_mut() {
            *b ^= 1;
        }
        Ok(v)
    }
    fn comprimir_com_base(&self, d: &[u8], b: &[u8]) -> nota_historico::Result<Vec<u8>> {
        self.0.comprimir_com_base(d, b)
    }
    fn descomprimir_com_base(
        &self,
        c: &[u8],
        b: &[u8],
        t: usize,
    ) -> nota_historico::Result<Vec<u8>> {
        self.0.descomprimir_com_base(c, b, t)
    }
}

#[test]
fn integridade_acusa_texto_que_nao_confere() {
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    let id = nota("a");
    let textos;
    {
        let mut h = Historico::abrir(&caminho).unwrap();
        textos = gravar_serie(&mut h, &id, 10, 11, 100);
    }
    let h = Historico::abrir_com(
        &caminho,
        Opcoes::default(),
        Box::new(CodecRuim(ZstdCodec::default())),
    )
    .unwrap();
    assert!(matches!(
        h.ler(&id, VersaoId(10)),
        Err(Erro::Integridade { versao: 10, .. })
    ));
    // Uma diferença antiga parte do quadro-chave estragado: o zstd pode não conseguir decodificar
    // (erro de codec), decodificar lixo (integridade) ou, se o byte estragado não foi usado,
    // reconstruir certo. O que não pode é devolver texto errado.
    for v in 1..=10u64 {
        if let Ok(t) = h.ler(&id, VersaoId(v)) {
            assert_eq!(
                &t,
                &textos[v as usize - 1],
                "versão {v} voltou errada sem erro"
            );
        }
    }
}

#[test]
fn banco_de_outro_codec_nao_abre() {
    struct Outro;
    impl Codec for Outro {
        fn nome(&self) -> &'static str {
            "outro"
        }
        fn comprimir(&self, d: &[u8]) -> nota_historico::Result<Vec<u8>> {
            Ok(d.to_vec())
        }
        fn descomprimir(&self, c: &[u8], _: usize) -> nota_historico::Result<Vec<u8>> {
            Ok(c.to_vec())
        }
        fn comprimir_com_base(&self, d: &[u8], _: &[u8]) -> nota_historico::Result<Vec<u8>> {
            Ok(d.to_vec())
        }
        fn descomprimir_com_base(
            &self,
            c: &[u8],
            _: &[u8],
            _: usize,
        ) -> nota_historico::Result<Vec<u8>> {
            Ok(c.to_vec())
        }
    }
    let d = tempfile::tempdir().unwrap();
    let caminho = d.path().join("h.redb");
    drop(Historico::abrir(&caminho).unwrap());
    assert!(matches!(
        Historico::abrir_com(&caminho, Opcoes::default(), Box::new(Outro)),
        Err(Erro::Argumento(_))
    ));
}
