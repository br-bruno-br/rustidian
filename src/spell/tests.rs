use super::*;

/// Dicionário de brinquedo (escrito para os testes; o VERO nunca é baixado aqui).
const MINI_AFF: &str = "\u{feff}SET UTF-8\nTRY aeiosrntlcdmpug\nSFX S Y 1\nSFX S 0 s .\n";
const MINI_DIC: &str =
    "\u{feff}12\ncasa/S\nação\nações\npão/S\nvocê\nsão\nnão\nBrasil\ngato/S\ntexto/S\nsó\nde\n";

fn engine() -> Arc<Engine> {
    Arc::new(Engine::new(MINI_AFF, MINI_DIC).expect("mini dicionário"))
}

fn checker() -> Checker {
    Checker::new(Some(engine()), Personal::default())
}

fn leak(language: Language) -> &'static Language {
    Box::leak(Box::new(language))
}

fn mini_language() -> &'static Language {
    let sha = |text: &str| -> &'static str {
        Box::leak(nota_sync::protocolo::sha256_hex(text.as_bytes()).into_boxed_str())
    };
    leak(Language {
        code: "mini",
        label: "mini",
        aff: Source {
            name: "mini.aff",
            url: "https://exemplo.invalido/mini.aff",
            sha256: sha(MINI_AFF),
        },
        dic: Source {
            name: "mini.dic",
            url: "https://exemplo.invalido/mini.dic",
            sha256: sha(MINI_DIC),
        },
    })
}

fn found<'a>(text: &'a str, list: &[Word]) -> Vec<&'a str> {
    list.iter().map(|word| &text[word.bytes.clone()]).collect()
}

fn wrong(text: &str) -> Vec<String> {
    let mut checker = checker();
    let all = words(text);
    let (flagged, _) = checker.flagged(text, &all, 0..usize::MAX, usize::MAX);
    flagged
        .iter()
        .map(|word| text[word.bytes.clone()].to_string())
        .collect()
}

#[test]
fn words_are_plain_prose_with_accents() {
    let text = "Você não tem ação, só casa.";
    let list = words(text);
    assert_eq!(
        found(text, &list),
        ["Você", "não", "tem", "ação", "só", "casa"]
    );
}

#[test]
fn byte_and_char_offsets_are_both_right_after_accents() {
    let text = "ação ação\nção ç é ñandú";
    let list = words(text);
    for word in &list {
        let by_bytes = &text[word.bytes.clone()];
        let by_chars: String = text
            .chars()
            .skip(word.chars.start)
            .take(word.chars.len())
            .collect();
        assert_eq!(by_bytes, by_chars);
    }
    let last = list.last().expect("palavras");
    assert_eq!(&text[last.bytes.clone()], "ñandú");
    assert!(
        last.bytes.start > last.chars.start,
        "bytes e caracteres divergem"
    );
    // Sem a letra solta `é` e `ç` (menos de 2 caracteres).
    assert_eq!(found(text, &list), ["ação", "ação", "ção", "ñandú"]);
}

#[test]
fn front_matter_is_skipped_even_with_broken_yaml() {
    let text = "---\ntitulo: xpto qwerty\ntags: [a\n---\nTexto xyzzy aqui\n";
    assert_eq!(found(text, &words(text)), ["Texto", "xyzzy", "aqui"]);
    let crlf = "---\r\ntitulo: abc\r\n---\r\nfoo\r\n";
    assert_eq!(found(crlf, &words(crlf)), ["foo"]);
    // Sem fechar: não é front matter.
    let open = "---\nsoltoo texto\n";
    assert_eq!(found(open, &words(open)), ["soltoo", "texto"]);
}

#[test]
fn code_fences_and_inline_code_are_skipped() {
    let text = "antes\n```rust\nlet xpto = 1;\n```\ndepois `qwertyuiop` e ``a`b zzzz`` fim\n~~~\nfoo bar\n~~~\n";
    assert_eq!(found(text, &words(text)), ["antes", "depois", "fim"]);
}

#[test]
fn math_is_skipped() {
    let text = "valor $xqz + yqz$ aqui\n\n$$\nabcd efgh\n$$\nfim";
    assert_eq!(found(text, &words(text)), ["valor", "aqui", "fim"]);
}

#[test]
fn links_urls_emails_and_files_are_skipped() {
    let text = "veja [[Nota Qwerty|apelido zzz]] e ![[img zzz.png]] e [texto bom](https://exemplo.com/a_b(c)/zzzz) \
        mais https://site.com/xyz www.site.com joao@exemplo.com arquivo.md src/main.rs <b>tag</b> [!note] ok";
    let list = words(text);
    assert_eq!(found(text, &list), ["veja", "texto", "bom", "mais", "ok"]);
    let reference = "[id]: https://exemplo.com/zzz yyy";
    assert!(words(reference).is_empty());
}

#[test]
fn numbers_digits_acronyms_camel_case_and_tags_are_skipped() {
    let text = "123 abc123 a1b ERP iPhone GitHub snake_case #etiqueta @pessoa ^bloco \\escape _itálico_ __forte__ palavra";
    assert_eq!(found(text, &words(text)), ["itálico", "forte", "palavra"]);
}

#[test]
fn hyphen_and_apostrophe_stay_inside_the_word() {
    let text = "casa-pão d'água rio–mar - solto";
    let list = words(text);
    assert_eq!(
        found(text, &list),
        ["casa-pão", "d'água", "rio", "mar", "solto"]
    );
    let proper = "Rio-Grande";
    assert_eq!(found(proper, &words(proper)), ["Rio-Grande"]);
}

#[test]
fn flags_only_unknown_words() {
    assert_eq!(wrong("Você não casaa sao gatos."), ["casaa", "sao"]);
}

#[test]
fn inflected_forms_and_hyphen_parts_are_accepted() {
    assert_eq!(wrong("casas gatos textos"), Vec::<String>::new());
    assert_eq!(wrong("pães"), ["pães"]);
    assert_eq!(wrong("casa-pão"), Vec::<String>::new());
    assert_eq!(wrong("casa-xyzw"), ["casa-xyzw"]);
}

#[test]
fn personal_list_loads_adds_and_persists() {
    let dir = tempfile::tempdir().expect("pasta");
    let path = dir.path().join("pessoal.txt");
    std::fs::write(&path, "Replay\nmaestro\n\n  \n").expect("escreve");
    let mut personal = Personal::open(path.clone());
    assert_eq!(personal.len(), 2);
    assert!(personal.contains("Replay"));
    assert!(personal.contains("REPLAY"));
    assert!(!personal.contains("replay"), "maiúscula é exata");
    assert!(personal.contains("maestro"));
    assert!(personal.contains("Maestro"));
    assert!(personal.contains("MAESTRO"));
    personal.add("zzxx").expect("grava");
    personal.add("zzxx").expect("repetida");
    personal.add("duas palavras").expect("ignora");
    let reread = std::fs::read_to_string(&path).expect("lê");
    assert!(reread.ends_with("zzxx\n"), "{reread:?}");
    assert_eq!(reread.matches("zzxx").count(), 1);
    assert!(Personal::open(path).contains("zzxx"));
}

#[test]
fn personal_add_breaks_a_line_that_has_no_final_newline() {
    let dir = tempfile::tempdir().expect("pasta");
    let path = dir.path().join("pessoal.txt");
    std::fs::write(&path, "um").expect("escreve");
    let mut personal = Personal::open(path.clone());
    personal.add("dois").expect("grava");
    assert_eq!(std::fs::read_to_string(path).expect("lê"), "um\ndois\n");
}

#[test]
fn checker_uses_personal_and_ignored_words_and_clears_the_cache() {
    let dir = tempfile::tempdir().expect("pasta");
    let mut checker = Checker::new(
        Some(engine()),
        Personal::open(dir.path().join("pessoal.txt")),
    );
    assert!(!checker.is_ok("replayy"));
    checker.add_personal("replayy").expect("grava");
    assert!(checker.is_ok("replayy"));
    assert!(!checker.is_ok("xqzw"));
    checker.ignore("xqzw");
    assert!(checker.is_ok("xqzw"));
    assert!(checker.is_ok("não"));
}

#[test]
fn verdicts_are_cached_per_word() {
    let text = "casa casa casa zzzz zzzz casa";
    let mut checker = checker();
    let all = words(text);
    let (first, incomplete) = checker.flagged(text, &all, 0..usize::MAX, usize::MAX);
    assert!(!incomplete);
    assert_eq!(first.len(), 2);
    assert_eq!(checker.lookups, 2, "uma consulta por palavra distinta");
    let (again, _) = checker.flagged(text, &all, 0..usize::MAX, usize::MAX);
    assert_eq!(again, first);
    assert_eq!(checker.lookups, 2, "a segunda passada saiu do cache");
}

#[test]
fn budget_stops_early_and_the_next_call_finishes() {
    let text = "aaaa bbbb cccc dddd eeee";
    let mut checker = checker();
    let all = words(text);
    let (partial, incomplete) = checker.flagged(text, &all, 0..usize::MAX, 2);
    assert!(incomplete);
    assert_eq!(partial.len(), 2);
    let (more, incomplete) = checker.flagged(text, &all, 0..usize::MAX, 2);
    assert!(incomplete);
    assert_eq!(more.len(), 4);
    let (done, incomplete) = checker.flagged(text, &all, 0..usize::MAX, 2);
    assert!(!incomplete);
    assert_eq!(done.len(), 5);
}

#[test]
fn only_visible_words_are_checked() {
    let text = "aaaa bbbb cccc dddd eeee ffff";
    let mut checker = checker();
    let all = words(text);
    // Caracteres 10..19: toca "cccc" (10..14) e "dddd" (15..19).
    let (flagged, _) = checker.flagged(text, &all, 10..19, usize::MAX);
    assert_eq!(found(text, &flagged), ["cccc", "dddd"]);
    assert_eq!(checker.lookups, 2);
}

#[test]
fn without_a_dictionary_nothing_is_flagged() {
    let mut checker = Checker::default();
    let text = "qualquer coisa zzzz";
    let (flagged, incomplete) = checker.flagged(text, &words(text), 0..100, 10);
    assert!(flagged.is_empty() && !incomplete);
}

#[test]
fn suggestions_come_from_the_dictionary() {
    let engine = engine();
    let suggestions = engine.suggest("casaa", 5);
    assert!(suggestions.contains(&"casa".to_string()), "{suggestions:?}");
    assert!(suggestions.len() <= 5);
}

#[test]
fn whole_word_suggestions_come_before_split_ones() {
    let engine = vec!["casa a", "casa-a", "cassa", "casas", "casa"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        order_suggestions("casaa", engine),
        ["cassa", "casas", "casa", "casa-a", "casa a"]
    );
    let kept = order_suggestions("guarda-chuvaa", vec!["guarda-chuva".into()]);
    assert_eq!(kept, ["guarda-chuva"]);
}

#[test]
fn engine_ignores_a_leading_bom() {
    assert!(Engine::new(MINI_AFF, MINI_DIC).is_ok());
    assert!(Engine::new("isto não é aff", "nem dic").is_err());
}

#[test]
fn download_checks_the_hash_before_writing_anything() {
    let language = mini_language();
    let dir = tempfile::tempdir().expect("pasta");
    let target = dir.path().join("dicionarios");
    let bad = |_: &str| Ok(b"conteudo errado".to_vec());
    let error = download(&target, language, &bad).expect_err("hash errado");
    assert!(error.contains("diferente"), "{error}");
    assert!(!target.exists(), "nada gravado");

    let good = |url: &str| {
        Ok(if url.ends_with(".aff") {
            MINI_AFF.as_bytes().to_vec()
        } else {
            MINI_DIC.as_bytes().to_vec()
        })
    };
    download(&target, language, &good).expect("baixa");
    assert!(installed(&target, language));
    assert!(Engine::load(&target, language).is_ok());
    assert!(!target.join("mini.aff.part").exists());

    let offline = |_: &str| Err("sem rede".to_string());
    let other = dir.path().join("outra");
    assert_eq!(download(&other, language, &offline), Err("sem rede".into()));
    assert!(!other.exists());
}

#[test]
fn pinned_pt_br_urls_and_hashes_are_well_formed() {
    for source in [&PT_BR.aff, &PT_BR.dic] {
        assert!(
            source
                .url
                .starts_with("https://raw.githubusercontent.com/LibreOffice/dictionaries/")
        );
        assert!(source.url.ends_with(&format!("/pt_BR/{}", source.name)));
        assert_eq!(source.sha256.len(), 64);
        assert!(
            source
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
    assert!(http_fetch("http://exemplo.invalido/x").is_err());
}

fn wait(service: &mut SpellService, until: impl Fn(&SpellService) -> bool) {
    for _ in 0..400 {
        service.poll();
        if until(service) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!(
        "o serviço não chegou ao estado esperado: {:?}",
        service.status()
    );
}

#[test]
fn service_downloads_loads_and_flags_in_the_background() {
    let language = mini_language();
    let dir = tempfile::tempdir().expect("pasta");
    let mut service = SpellService::new(Some(dir.path().join("d")), language);
    assert!(!service.active());
    // Desligado: não carrega nada.
    service.poll();
    assert_eq!(service.status(), &Status::Idle);

    service.set_enabled(true);
    service.poll();
    assert_eq!(
        service.status(),
        &Status::Missing,
        "sem arquivos, pede o download"
    );
    assert!(service.flagged("zzzz", 0..10, 10).0.is_empty());

    service.start_download_with(|url| {
        Ok(if url.ends_with(".aff") {
            MINI_AFF.as_bytes().to_vec()
        } else {
            MINI_DIC.as_bytes().to_vec()
        })
    });
    assert_eq!(service.status(), &Status::Downloading);
    wait(&mut service, SpellService::active);

    let text = "casa zzzz";
    let (flagged, _) = service.flagged(text, 0..100, 10);
    assert_eq!(found(text, &flagged), ["zzzz"]);
    assert!(service.suggestions("casaa").contains(&"casa".to_string()));

    service.ignore("zzzz");
    assert!(service.flagged(text, 0..100, 10).0.is_empty());
    service.add_to_personal("qqqq").expect("grava");
    assert!(service.flagged("qqqq", 0..100, 10).0.is_empty());
    assert!(personal_file(&dir.path().join("d"), language).is_file());

    service.set_enabled(false);
    assert!(!service.active());
}

#[test]
fn a_failed_download_keeps_the_service_usable_and_explains() {
    let language = mini_language();
    let dir = tempfile::tempdir().expect("pasta");
    let mut service = SpellService::new(Some(dir.path().join("d")), language);
    service.set_enabled(true);
    service.poll();
    service.start_download_with(|_| Err("sem conexão".into()));
    wait(&mut service, |s| !s.busy());
    assert_eq!(service.status(), &Status::Missing);
    assert_eq!(service.message.as_deref(), Some("sem conexão"));
    assert!(!service.active());
}

#[test]
fn a_new_text_is_tokenized_again_and_the_same_text_is_not() {
    let language = mini_language();
    let dir = tempfile::tempdir().expect("pasta");
    std::fs::write(dir.path().join("mini.aff"), MINI_AFF).expect("aff");
    std::fs::write(dir.path().join("mini.dic"), MINI_DIC).expect("dic");
    let mut service = SpellService::new(Some(dir.path().to_path_buf()), language);
    service.set_enabled(true);
    wait(&mut service, SpellService::active);
    let (first, _) = service.flagged("casa zzzz", 0..100, 10);
    assert_eq!(first.len(), 1);
    let (second, _) = service.flagged("casa zzzz yyyy", 0..100, 10);
    assert_eq!(second.len(), 2);
}

/// Com o VERO de verdade (baixado antes, fora do teste): `SPELL_TEST_DIR=<pasta> cargo test
/// --release real_pt_br -- --ignored --nocapture`.
#[test]
#[ignore = "precisa de pt_BR.aff e pt_BR.dic em SPELL_TEST_DIR"]
fn real_pt_br_dictionary() {
    let dir = PathBuf::from(std::env::var("SPELL_TEST_DIR").expect("SPELL_TEST_DIR"));
    let started = std::time::Instant::now();
    let engine = Engine::load(&dir, &PT_BR).expect("carrega o VERO");
    println!("carga: {:?}", started.elapsed());
    for word in [
        "casa",
        "ação",
        "você",
        "português",
        "Brasil",
        "computador",
        "guarda-chuva",
    ] {
        assert!(engine_accepts(&engine, word), "{word}");
    }
    for word in ["casaa", "informatca", "xpto", "pasaro"] {
        assert!(!engine_accepts(&engine, word), "{word}");
    }
    let started = std::time::Instant::now();
    for typo in ["informatca", "pasaro", "computadro", "casaa", "obrigdo"] {
        let suggestions = engine.suggest(typo, 5);
        println!("{typo}: {suggestions:?}");
    }
    println!("sugestões (5 palavras) em {:?}", started.elapsed());
    let suggestions = engine.suggest("computadro", 5);
    assert!(
        suggestions.iter().any(|s| s == "computador"),
        "{suggestions:?}"
    );
    let text = "Eu gosto de programar e tenho uma casa no Brasil. Ontem fui ao mercado comprar pão, leite e ovos.\n".repeat(2000);
    let all = words(&text);
    let mut checker = Checker::new(Some(Arc::new(engine)), Personal::default());
    let started = std::time::Instant::now();
    let (flagged, incomplete) = checker.flagged(&text, &all, 0..usize::MAX, usize::MAX);
    println!(
        "{} palavras, {} erradas, {} consultas em {:?}",
        all.len(),
        flagged.len(),
        checker.lookups,
        started.elapsed()
    );
    assert!(!incomplete);
    assert!(flagged.is_empty(), "{:?}", found(&text, &flagged));
}
