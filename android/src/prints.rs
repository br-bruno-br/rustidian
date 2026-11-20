//! Verificação visual sem celular: renderiza as telas reais em HTML (dioxus-ssr). Os testes
//! comuns conferem o conteúdo; o teste `#[ignore]` grava os HTML em uma pasta para o
//! Chrome headless fotografar (390x844, claro e escuro). Nunca abre janela.
//!
//!   cargo test gravar_html_das_telas -- --ignored
//! (a pasta sai de `RUSTIDIAN_PRINTS`, ou `C:\dev\_wt\prints-r7b3\html`)

use std::fs;
use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use crate::logica::ajustes::{Ajustes, Arquivos};
use crate::logica::cofre::Cofre;
use crate::logica::local::Local;
use crate::logica::sincronia::{EstadoSync, Fase, explicar_erro};
use crate::rota::Rota;
use crate::servicos::Servicos;
use crate::ui::dialogo::{DialogoConfirmar, DialogoTexto};
use crate::ui::{CSS, Raiz, RaizProps};

const NOTA_DE_EXEMPLO: &str = "# Ideias de app\n\nUm caderno que abre rápido e nunca perde o que escrevi.\n\n- sincronizar com o servidor de casa\n- pastas e busca\n- funcionar sem internet\n\nAção, pão, açúcar e café: os acentos vêm do teclado do celular, como numa página qualquer.\n\n## Para testar\n\nEscrever uma nota grande, girar a tela e ver se o teclado não cobre a última linha.\n";

/// Nota de exemplo da Leitura: título, texto com links, tarefas, tabela.
const NOTA_REUNIAO: &str = "# Reunião de sexta\n\nPauta da **reunião** com o *time*. Veja o [[Projeto Alfa]] (ou [[Projeto Alfa|o projeto]]) e o [site da loja](https://exemplo.com/docs).\n\n## Tarefas\n\n- [x] Mandar a ata\n- [ ] Ligar para o fornecedor\n- [ ] Revisar o orçamento\n  - [x] Conferir os cabos\n\n## Orçamento\n\n| Item | Qtd | Valor |\n|:--|:-:|--:|\n| Cabos | 4 | R$ 80,00 |\n| Placa | 1 | R$ 240,00 |\n\n1. Aprovar\n2. Comprar\n3. Entregar\n";

/// Segunda nota de exemplo: código, citação, rodapé e o que NÃO pode rodar.
const NOTA_CODIGO: &str = "# Código e segurança\n\nUm trecho em `Rust`:\n\n```rust\nfn soma(a: i32, b: i32) -> i32 {\n    a + b\n}\n```\n\n> Tachado fica ~~assim~~ e a citação, assim.\n\nTexto com nota de rodapé[^1].\n\n## O que não pode rodar\n\n<script>alert(1)</script>\n\nNo meio da frase <img src=x onerror=alert(1)> aparece como texto. [Este link](javascript:alert(1)) não é link. ![foto](https://exemplo.com/foto.png)\n\n[^1]: Veja a ata de sexta.\n";

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
    c.salvar_nota("Reunião de sexta.md", NOTA_REUNIAO).unwrap();
    c.salvar_nota("Código e segurança.md", NOTA_CODIGO).unwrap();
    c.salvar_nota("Projetos/Projeto Alfa.md", "# Projeto Alfa\n")
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

/// Um cofre de exemplo com a sincronização num estado escolhido (sem servidor nenhum).
fn servicos_com_sync(
    raiz: &Path,
    ajustes: Option<Ajustes>,
    estado: Option<EstadoSync>,
) -> Servicos {
    if let Some(a) = ajustes {
        Arquivos::em(raiz.with_file_name("estado-sync"))
            .salvar(&a)
            .unwrap();
    }
    let servicos = cofre_de_exemplo(raiz);
    // `cofre_de_exemplo` já abriu a Sincronia (e leu a configuração gravada acima).
    if let Some(e) = estado {
        servicos.sync.definir_estado_para_teste(e);
    }
    servicos
}

fn ajustes_conectado() -> Ajustes {
    Ajustes {
        servidor: "http://100.88.231.30:8443".into(),
        aparelho: "Pixel do Bruno".into(),
        token: "nst_exemplo".into(),
        conectado: true,
    }
}

fn hoje(h: u32, m: u32) -> Option<chrono::NaiveDateTime> {
    chrono::Local::now().date_naive().and_hms_opt(h, m, 0)
}

fn estado_ok() -> EstadoSync {
    EstadoSync {
        fase: Fase::Sincronizado,
        ultima: hoje(14, 32),
        ciclos: 3,
        ..EstadoSync::default()
    }
}

fn estado_sem_conexao() -> EstadoSync {
    let (fase, texto, motivo) = explicar_erro(&nota_sync::ErroSync::Remoto(
        nota_sync::ErroRemoto::Rede("connection timed out".into()),
    ));
    EstadoSync {
        fase,
        ultima: hoje(9, 5),
        erro: Some(texto),
        motivo_rede: motivo,
        ciclos: 2,
        ..EstadoSync::default()
    }
}

fn estado_com_conflitos() -> EstadoSync {
    EstadoSync {
        conflitos: vec![
            "Reunião de sexta (conflito Pixel do Bruno 2026-11-20 1432).md".into(),
            "Projetos/Projeto Alfa (conflito Pixel do Bruno 2026-11-20 1432).md".into(),
        ],
        ..estado_ok()
    }
}

fn sincronizar(servicos: Servicos) -> String {
    renderizar(servicos, Some(Rota::Sincronizar))
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
    // O botão que leva para a leitura formatada.
    assert!(html.contains("aria-label=\"Ler a nota formatada\""));
}

fn leitura(servicos: Servicos, caminho: &str) -> String {
    renderizar(
        servicos,
        Some(Rota::Leitura {
            caminho: caminho.into(),
        }),
    )
}

#[test]
fn leitura_mostra_a_nota_formatada() {
    let t = tempfile::tempdir().unwrap();
    let html = leitura(
        cofre_de_exemplo(&t.path().join("Cofre")),
        "Reunião de sexta.md",
    );
    for esperado in [
        "<h1>Reunião de sexta</h1>",
        "<strong>reunião</strong>",
        "<h2>Tarefas</h2>",
        "aria-checked=\"true\" data-t=\"0\"",
        "aria-checked=\"false\" data-t=\"1\"",
        "<table>",
        "<th class=\"a-d\">Valor</th>",
        "<ol>",
        // Link para outra nota do cofre e link da web: sem href, só o índice.
        "class=\"link-nota\"",
        "class=\"link-web\"",
        "(exemplo.com)",
        // Cabeçalho da tela: voltar, título, alternar para o editor.
        "aria-label=\"Voltar\"",
        "aria-label=\"Editar a nota\"",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
    assert!(
        !html.contains("href"),
        "a Leitura não tem endereços: {html}"
    );
    assert!(!html.contains("<textarea"));
}

#[test]
fn leitura_mostra_html_cru_como_texto_e_nunca_executa() {
    let t = tempfile::tempdir().unwrap();
    let html = leitura(
        cofre_de_exemplo(&t.path().join("Cofre")),
        "Código e segurança.md",
    );
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{html}"
    );
    assert!(
        html.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "{html}"
    );
    assert!(html.contains("imagem: foto.png"));
    assert!(html.contains("Este link"));
    for perigoso in ["<script", "<img", "javascript:", "href"] {
        assert!(!html.contains(perigoso), "{perigoso:?} em {html}");
    }
    // Código, citação e rodapé.
    assert!(html.contains("class=\"lang-rust\""));
    assert!(html.contains("<blockquote>"));
    assert!(html.contains("<del>assim</del>"));
    assert!(html.contains("rodape-notas"));
}

#[test]
fn leitura_de_nota_vazia_e_de_nota_que_sumiu() {
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    let html = leitura(servicos.clone(), "Sem título.md");
    assert!(html.contains("Esta nota está vazia"));
    let html = leitura(servicos, "nao-existe.md");
    assert!(html.contains("Não encontrei o arquivo"));
    assert!(!html.contains("class=\"leitura\""));
}

#[test]
fn lista_mostra_o_rotulo_da_sincronizacao_que_leva_a_tela() {
    let t = tempfile::tempdir().unwrap();
    let html = renderizar(cofre_de_exemplo(&t.path().join("Cofre")), None);
    assert!(html.contains("class=\"chip tom-neutro\""), "{html}");
    assert!(html.contains("Sincronização desligada"));
    let t = tempfile::tempdir().unwrap();
    let html = renderizar(
        servicos_com_sync(
            &t.path().join("Cofre"),
            Some(ajustes_conectado()),
            Some(estado_ok()),
        ),
        None,
    );
    assert!(html.contains("class=\"chip tom-bom\""), "{html}");
    assert!(html.contains("Sincronizado 14:32"));
    // Dentro de uma subpasta o rótulo não aparece (só na raiz).
    let t = tempfile::tempdir().unwrap();
    let html = renderizar(
        cofre_de_exemplo(&t.path().join("Cofre")),
        Some(Rota::Lista {
            pasta: "Diário".into(),
        }),
    );
    assert!(!html.contains("class=\"chip"));
}

#[test]
fn sincronizar_vazia_mostra_o_formulario_e_o_aviso_da_tailscale() {
    let t = tempfile::tempdir().unwrap();
    let html = sincronizar(cofre_de_exemplo(&t.path().join("Cofre")));
    for esperado in [
        "Sincronização desligada",
        "Só funciona pela Tailscale ou na rede de casa",
        "http:// (sem HTTPS)",
        "Endereço do servidor",
        "Nome deste aparelho",
        "Token do aparelho",
        "Frase-senha do cofre",
        "não fica guardada",
        "Conectar",
        "aria-label=\"Voltar\"",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
    // Token e frase são campos de senha e nascem vazios.
    assert_eq!(html.matches("type=\"password\"").count(), 2);
    assert!(!html.contains("Sincronizar agora"));
    assert!(!html.contains("Desconectar"));
}

#[test]
fn sincronizar_configurada_mostra_estado_e_botoes() {
    let t = tempfile::tempdir().unwrap();
    let html = sincronizar(servicos_com_sync(
        &t.path().join("Cofre"),
        Some(ajustes_conectado()),
        Some(estado_ok()),
    ));
    for esperado in [
        "Sincronizado 14:32",
        "Última sincronização: hoje 14:32",
        "http://100.88.231.30:8443",
        "Pixel do Bruno",
        "Sincronizar agora",
        "Desconectar",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
    assert!(!html.contains("Frase-senha do cofre"));
    assert!(!html.contains("Conflitos ("));
    // O token nunca é desenhado na tela.
    assert!(!html.contains("nst_exemplo"));
}

#[test]
fn sincronizar_com_erro_explica_em_linguagem_simples() {
    let t = tempfile::tempdir().unwrap();
    let html = sincronizar(servicos_com_sync(
        &t.path().join("Cofre"),
        Some(ajustes_conectado()),
        Some(estado_sem_conexao()),
    ));
    for esperado in [
        "Sem conexão",
        "Confira se a Tailscale está ligada neste celular",
        "Detalhe técnico: connection timed out",
        "Última sincronização: hoje 09:05",
        "Sincronizar agora",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
}

#[test]
fn sincronizar_pede_a_frase_depois_de_reabrir_e_mostra_o_erro_da_frase() {
    let t = tempfile::tempdir().unwrap();
    let estado = EstadoSync {
        erro: Some(crate::logica::conexao::ErroConexao::FraseErrada.to_string()),
        ..EstadoSync::default()
    };
    let html = sincronizar(servicos_com_sync(
        &t.path().join("Cofre"),
        Some(ajustes_conectado()),
        Some(EstadoSync {
            fase: Fase::PedeFrase,
            ..estado
        }),
    ));
    for esperado in [
        "Digite a frase para sincronizar",
        "Frase-senha incorreta",
        "Entrar",
        "Desconectar",
        "Conectado a http://100.88.231.30:8443",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
    assert!(!html.contains("Token do aparelho"));
    assert_eq!(html.matches("type=\"password\"").count(), 1);
}

#[test]
fn sincronizar_com_conflitos_lista_as_copias() {
    let t = tempfile::tempdir().unwrap();
    let html = sincronizar(servicos_com_sync(
        &t.path().join("Cofre"),
        Some(ajustes_conectado()),
        Some(estado_com_conflitos()),
    ));
    for esperado in [
        "2 conflitos",
        "Conflitos (2)",
        "Reunião de sexta (conflito Pixel do Bruno 2026-11-20 1432)",
        "Projeto Alfa (conflito Pixel do Bruno 2026-11-20 1432)",
        "As duas versões foram guardadas",
    ] {
        assert!(html.contains(esperado), "faltou {esperado:?} em {html}");
    }
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
        .unwrap_or_else(|| PathBuf::from(r"C:\dev\_wt\prints-r7b3\html"));
    fs::create_dir_all(&pasta).unwrap();
    let t = tempfile::tempdir().unwrap();
    let servicos = cofre_de_exemplo(&t.path().join("Cofre"));
    // Um cofre para cada estado da sincronização (cada um com a sua pasta de estado).
    let com_sync = |nome: &str, ajustes: Option<Ajustes>, estado: Option<EstadoSync>| {
        servicos_com_sync(&t.path().join(nome).join("Cofre"), ajustes, estado)
    };
    let telas: Vec<(&str, String)> = vec![
        ("lista", renderizar(servicos.clone(), None)),
        (
            "lista-sincronizada",
            renderizar(
                com_sync("s0", Some(ajustes_conectado()), Some(estado_ok())),
                None,
            ),
        ),
        ("sincronizar-vazia", sincronizar(servicos.clone())),
        (
            "sincronizar-ok",
            sincronizar(com_sync("s1", Some(ajustes_conectado()), Some(estado_ok()))),
        ),
        (
            "sincronizar-erro",
            sincronizar(com_sync(
                "s2",
                Some(ajustes_conectado()),
                Some(estado_sem_conexao()),
            )),
        ),
        (
            "sincronizar-conflito",
            sincronizar(com_sync(
                "s3",
                Some(ajustes_conectado()),
                Some(estado_com_conflitos()),
            )),
        ),
        (
            "sincronizar-frase",
            sincronizar(com_sync(
                "s4",
                Some(ajustes_conectado()),
                Some(EstadoSync {
                    fase: Fase::PedeFrase,
                    erro: Some(crate::logica::conexao::ErroConexao::FraseErrada.to_string()),
                    ..EstadoSync::default()
                }),
            )),
        ),
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
                servicos.clone(),
                Some(Rota::Nota {
                    caminho: "Ideias de app.md".into(),
                }),
            ),
        ),
        ("leitura", leitura(servicos.clone(), "Reunião de sexta.md")),
        ("leitura-codigo", leitura(servicos, "Código e segurança.md")),
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
