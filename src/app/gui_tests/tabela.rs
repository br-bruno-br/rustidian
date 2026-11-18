//! R13: edição de tabela com conforto, sem janela (egui_kittest). Cobre o que a fatia trouxe:
//! criar tabela pela paleta com colunas x linhas, colar dados de planilha (TSV) fora e dentro de
//! uma tabela, converter a seleção em tabela e alinhar as barras do Markdown. O teclado entre
//! células (Tab, Shift+Tab, Enter e Tab na última célula) já tem teste em `edicao.rs`.

use super::edicao::{NONE, SHIFT};
use super::*;

const NOTA: &str = "# Tabelas\n\nantes\n";

impl Session {
    /// Abre `Tab.md` no modo pedido e o cursor no fim.
    fn abrir_tabela(&mut self, modo: &str) {
        self.edit("Tab", modo);
    }

    /// Roda um comando da paleta de tabelas e espera o editor aplicá-lo.
    fn comando_de_tabela(&mut self, nome: &str) {
        self.command(nome);
        self.settle();
    }
}

fn sessao(conteudo: &str) -> Session {
    Session::open_with(&[("Tab.md", conteudo.as_bytes())])
}

#[test]
fn a_paleta_cria_uma_tabela_vazia_com_colunas_x_linhas() {
    let mut session = sessao(NOTA);
    session.abrir_tabela("Source editing");
    session.comando_de_tabela("Tables: Insert table");
    assert!(
        session.has_label("Inserir tabela"),
        "o diálogo de tamanho abre"
    );
    session.select_all();
    session.type_text("2x3");
    session.key(NONE, egui::Key::Enter);
    session.settle();
    let esperado = format!(
        "{NOTA}\n|     |     |\n| --- | --- |\n|     |     |\n|     |     |\n",
        NOTA = NOTA
    );
    assert_eq!(session.active_text(), esperado);
    // Um tamanho fora do limite não cria nada e o diálogo continua aberto, com o motivo.
    let mut session = sessao(NOTA);
    session.abrir_tabela("Source editing");
    session.comando_de_tabela("Tables: Insert table");
    session.select_all();
    session.type_text("99x2");
    session.key(NONE, egui::Key::Enter);
    session.settle();
    assert_eq!(session.active_text(), NOTA);
    assert!(session.has_label("Use de 1 a"), "o limite é explicado");
}

#[test]
fn a_tabela_nova_nasce_com_o_cursor_na_primeira_celula_do_cabecalho() {
    let mut session = sessao(NOTA);
    session.abrir_tabela("Source editing");
    session.comando_de_tabela("Tables: Insert table");
    session.select_all();
    session.type_text("2x2");
    session.key(NONE, egui::Key::Enter);
    session.settle();
    session.type_text("Nome");
    let texto = session.active_text();
    assert!(
        texto.contains("| Nome    |     |\n| --- | --- |"),
        "o texto digitado entra na primeira célula: {texto:?}"
    );
}

#[test]
fn colar_linhas_de_planilha_no_texto_vira_tabela_e_um_desfazer_desfaz_tudo() {
    let mut session = sessao(NOTA);
    session.abrir_tabela("Source editing");
    session.paste("Nome\tQtd\r\nAna\t3\r\nBia\t12\r\n");
    let esperado = format!(
        "{NOTA}\n| Nome | Qtd |\n| ---- | --- |\n| Ana  | 3   |\n| Bia  | 12  |\n",
        NOTA = NOTA
    );
    assert_eq!(session.active_text(), esperado);
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), NOTA, "um Ctrl+Z desfaz a colagem");
}

#[test]
fn colar_texto_comum_ou_codigo_com_tabulacao_nao_vira_tabela() {
    let mut session = sessao(NOTA);
    session.abrir_tabela("Source editing");
    session.paste("uma linha só, sem tabulação");
    session.paste("a\tb\tc\n");
    assert_eq!(
        session.active_text(),
        format!("{NOTA}uma linha só, sem tabulaçãoa\tb\tc\n")
    );
    // Dentro de uma cerca de código a tabulação é do código.
    let codigo = "```\nx\n";
    let mut session = sessao(codigo);
    session.abrir_tabela("Source editing");
    session.paste("a\tb\nc\td\n");
    assert_eq!(session.active_text(), format!("{codigo}a\tb\nc\td\n"));
}

#[test]
fn a_paleta_converte_a_selecao_separada_por_ponto_e_virgula_em_tabela() {
    let nota = "# C\n\nnome;nota\nAna;9\n";
    let mut session = sessao(nota);
    session.abrir_tabela("Source editing");
    // O cursor está no fim; sobe duas linhas selecionando.
    session.key(SHIFT, egui::Key::ArrowUp);
    session.key(SHIFT, egui::Key::ArrowUp);
    session.comando_de_tabela("Tables: Convert selection to table");
    assert_eq!(
        session.active_text(),
        "# C\n\n| nome | nota |\n| ---- | ---- |\n| Ana  | 9    |\n"
    );
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), nota, "desfazer devolve o texto");
}

#[test]
fn a_paleta_alinha_as_barras_da_tabela_sob_o_cursor_e_depois_a_nota_inteira() {
    let nota = "|a|b|\n|-|-|\n|1|2|\n\nmeio\n\n| Item | Valor |\n| :--- | ---: |\n| x | 10 |\n";
    let mut session = sessao(nota);
    session.abrir_tabela("Source editing");
    // O cursor está no fim da nota, dentro da segunda tabela.
    session.comando_de_tabela("Tables: Align the table at the cursor");
    let so_a_segunda =
        "|a|b|\n|-|-|\n|1|2|\n\nmeio\n\n| Item | Valor |\n| :--- | ----: |\n| x    |    10 |\n";
    assert_eq!(session.active_text(), so_a_segunda);
    session.comando_de_tabela("Tables: Align all tables in the note");
    let todas = "| a   | b   |\n| --- | --- |\n| 1   | 2   |\n\nmeio\n\n| Item | Valor |\n| :--- | ----: |\n| x    |    10 |\n";
    assert_eq!(session.active_text(), todas);
    // Já alinhado: nada muda e a nota não fica suja por causa do comando.
    session.key(CTRL, egui::Key::S);
    session.comando_de_tabela("Tables: Align all tables in the note");
    assert_eq!(session.active_text(), todas);
    assert!(
        !session.app().tabs[session.app().active].document.dirty(),
        "um alinhamento sem efeito não suja a nota"
    );
}

const TABELA: &str = "# Colar\n\n| A | B |\n| --- | --- |\n| old | 1 |\n| other | 2 |\n\nDepois\n";

#[test]
fn colar_um_bloco_de_planilha_numa_celula_preenche_a_partir_dela_e_cresce_a_tabela() {
    let mut session = sessao(TABELA);
    session.open_note("Tab");
    session.command("Live preview");
    session.settle();
    session.click_value("old");
    session.paste("x\ty\r\nz\tw\r\nq\tr\r\n");
    session.settle();
    let esperado =
        "# Colar\n\n| A | B |\n| --- | --- |\n| x | y |\n| z | w |\n| q | r |\n\nDepois\n";
    assert_eq!(session.active_text(), esperado);
    // A colagem é um passo só no desfazer do documento.
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), TABELA);
}

#[test]
fn colar_na_celula_com_a_tabulacao_a_direita_cria_colunas() {
    let mut session = sessao(TABELA);
    session.open_note("Tab");
    session.command("Live preview");
    session.settle();
    session.click_value("1");
    session.paste("p\tq\r\n");
    session.settle();
    assert_eq!(
        session.active_text(),
        "# Colar\n\n| A | B |  |\n| --- | --- | --- |\n| old | p | q |\n| other | 2 |  |\n\nDepois\n"
    );
}

#[test]
fn texto_comum_colado_numa_celula_continua_sendo_texto_da_celula() {
    let mut session = sessao(TABELA);
    session.open_note("Tab");
    session.command("Live preview");
    session.settle();
    session.click_value("old");
    session.select_all();
    session.paste("novo");
    session.settle();
    assert_eq!(session.active_text(), TABELA.replace("| old |", "| novo |"));
}
