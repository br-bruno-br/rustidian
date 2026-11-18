//! Verificação visual sem celular: renderiza as telas reais em HTML (dioxus-ssr). Os testes
//! comuns conferem o conteúdo; o teste `#[ignore]` grava os HTML em uma pasta para o
//! Chrome headless fotografar (390x844, claro e escuro). Nunca abre janela.
//!
//!   cargo test gravar_html_das_telas -- --ignored
//! (a pasta sai de `RUSTIDIAN_PRINTS`, ou `C:\dev\_wt\prints-r7b1\html`)

use std::fs;
use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use crate::logica::cofre::Cofre;
use crate::logica::local::Local;
use crate::rota::Rota;
use crate::servicos::Servicos;
use crate::ui::dialogo::{DialogoConfirmar, DialogoTexto};
use crate::ui::{CSS, Raiz, RaizProps};

const NOTA_DE_EXEMPLO: &str = "# Ideias de app\n\nUm caderno que abre rápido e nunca perde o que escrevi.\n\n- sincronizar com o servidor de casa\n- pastas e busca\n- funcionar sem internet\n\nAção, pão, açúcar e café: os acentos vêm do teclado do celular, como numa página qualquer.\n\n## Para testar\n\nEscrever uma nota grande, girar a tela e ver se o teclado não cobre a última linha.\n";

fn cofre_de_exemplo(raiz: &Path) -> Servicos {
    let c = Cofre::abrir(raiz.to_path_buf()).unwrap();
    for pasta in ["Diário", "Projetos", "Receitas"] {
        c.criar_pasta("", pasta).unwrap();
    }
    c.salvar_nota("Compras.md", "# Compras\n\n- leite\n- pão\n- café\n")
        .unwrap();
    c.salvar_nota("Ideias de app.md", NOTA_DE_EXEMPLO).unwrap();
    c.salvar_nota("Lista de leitura.md", "Livros para ler em 2027.\n")
        .unwrap();
    c.salvar_nota("Sem título.md", "").unwrap();
    c.salvar_nota("Diário/2026-11-18.md", "Hoje testei o app no celular.\n")
        .unwrap();
    Servicos::de_local(Local {
        raiz: raiz.to_path_buf(),
        aviso: None,
    })
    .unwrap()
}

fn renderizar(servicos: Servicos, inicial: Option<Rota>) -> String {
    let mut vdom = VirtualDom::new_with_props(Raiz, RaizProps { servicos, inicial });
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

#[component]
fn DemoDialogos() -> Element {
    rsx! {
        div { class: "tela",
            DialogoTexto {
                titulo: "Nova pasta",
                rotulo_ok: "Criar",
                valor: "Nova pasta",
                on_ok: |_| {},
                on_cancelar: |_| {},
            }
        }
    }
}

#[component]
fn DemoConfirmar() -> Element {
    rsx! {
        div { class: "tela",
            DialogoConfirmar {
                titulo: "Mover a nota “Compras” para a lixeira?",
                mensagem: "Ela vai para a lixeira do cofre (.trash). Nada é apagado de vez.",
                rotulo_ok: "Mover para a lixeira",
                on_ok: |_| {},
                on_cancelar: |_| {},
            }
        }
    }
}

fn renderizar_componente(app: fn() -> Element) -> String {
    let mut vdom = VirtualDom::new(app);
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

/// Página completa para o navegador (o `viewport` do app já vem no HTML, como no celular).
fn pagina(titulo: &str, corpo: &str) -> String {
    format!(
        "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{titulo}</title></head><body>{corpo}</body></html>"
    )
}

#[test]
fn lista_da_raiz_mostra_pastas_notas_e_acoes() {
    let t = tempfile::tempdir().unwrap();
    let html = renderizar(cofre_de_exemplo(&t.path().join("Cofre")), None);
    for esperado in [
        "Diário",
        "Projetos",
        "Receitas",
        "Compras",
        "Ideias de app",
        "Nova nota",
        "Nova pasta",
        "Buscar por nome ou texto",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
    // As pastas vêm antes das notas e a lixeira não aparece.
    assert!(html.find("Diário").unwrap() < html.find("Compras").unwrap());
    assert!(!html.contains(".trash"));
    // CSS e tema do sistema vão junto.
    assert!(html.contains("prefers-color-scheme: dark"));
}

#[test]
fn lista_de_subpasta_tem_voltar_e_so_o_que_esta_nela() {
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    let html = renderizar(
        servicos,
        Some(Rota::Lista {
            pasta: "Diário".into(),
        }),
    );
    assert!(html.contains("2026-11-18"));
    assert!(html.contains("aria-label=\"Voltar\""));
    assert!(!html.contains("Compras"));
}

#[test]
fn cofre_vazio_explica_o_que_fazer() {
    let t = tempfile::tempdir().unwrap();
    let servicos = Servicos::de_local(Local {
        raiz: t.path().join("Cofre"),
        aviso: None,
    })
    .unwrap();
    let html = renderizar(servicos, None);
    assert!(html.contains("Seu cofre está vazio"));
}

#[test]
fn nota_abre_com_o_texto_e_indicador_salvo() {
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    let html = renderizar(
        servicos,
        Some(Rota::Nota {
            caminho: "Ideias de app.md".into(),
        }),
    );
    assert!(html.contains("<textarea"));
    assert!(html.contains("Um caderno que abre rápido"));
    assert!(html.contains("Ideias de app"));
    assert!(html.contains(">Salvo<"));
    assert!(html.contains("Renomear a nota"));
}

#[test]
fn nota_que_sumiu_mostra_o_erro_sem_campo() {
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    let html = renderizar(
        servicos,
        Some(Rota::Nota {
            caminho: "nao-existe.md".into(),
        }),
    );
    assert!(html.contains("Não encontrei o arquivo"));
    assert!(!html.contains("<textarea"));
}

#[test]
#[ignore = "grava os HTML para o Chrome headless fotografar"]
fn gravar_html_das_telas() {
    let pasta = std::env::var_os("RUSTIDIAN_PRINTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\dev\_wt\prints-r7b1\html"));
    fs::create_dir_all(&pasta).unwrap();
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    let telas: Vec<(&str, String)> = vec![
        ("lista", renderizar(servicos.clone(), None)),
        (
            "subpasta",
            renderizar(
                servicos.clone(),
                Some(Rota::Lista {
                    pasta: "Diário".into(),
                }),
            ),
        ),
        (
            "nota",
            renderizar(
                servicos,
                Some(Rota::Nota {
                    caminho: "Ideias de app.md".into(),
                }),
            ),
        ),
        (
            "dialogo-pasta",
            format!(
                "<style>{CSS}</style>{}",
                renderizar_componente(DemoDialogos)
            ),
        ),
        (
            "dialogo-lixeira",
            format!(
                "<style>{CSS}</style>{}",
                renderizar_componente(DemoConfirmar)
            ),
        ),
    ];
    for (nome, corpo) in telas {
        fs::write(pasta.join(format!("{nome}.html")), pagina(nome, &corpo)).unwrap();
    }
}
