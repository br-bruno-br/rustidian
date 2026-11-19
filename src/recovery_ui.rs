use crate::{theme, ui_probe};
use eframe::egui::{
    self, Align, FontId, Layout, RichText, ScrollArea, TextEdit,
    text::{LayoutJob, TextFormat},
};
use rustidian::{
    historico_fundo::{
        ApagadoEmTodas, Historico, Leitor, Origem, Tipo, Trecho, VersaoId, VersaoInfo,
        diff_palavras,
    },
    vault::{Document, Vault},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::SystemTime,
};

#[derive(Clone)]
pub struct Restore {
    pub expected: Document,
    /// O texto da versão, já lido para a tela: restaurar não espera o banco.
    pub text: String,
}

/// Uma linha da lista: a versão e quanto o texto cresceu (ou encolheu) em relação à anterior.
#[derive(Clone)]
struct Linha {
    info: VersaoInfo,
    delta: i64,
}

struct Loaded {
    files: Option<Vec<PathBuf>>,
    path: Option<PathBuf>,
    versions: Option<Vec<Linha>>,
    selected: Option<u64>,
    text: Option<String>,
    changes: Vec<Trecho>,
}

/// O pedido de apagar um trecho que está rodando: numa nota ou em todas.
enum Apagando {
    Nota(Receiver<Result<usize, String>>),
    Todas(Receiver<Result<ApagadoEmTodas, String>>),
}

/// "1 versão" ou "5 versões".
fn versoes_por_extenso(quantas: usize) -> String {
    match quantas {
        1 => "1 versão".into(),
        n => format!("{n} versões"),
    }
}

/// Altura de cada linha da lista de versões. `show_rows` precisa dela igual à altura real do
/// botão, senão o deslocamento calculado por índice não bate com o que está na tela.
const LINHA: f32 = theme::space::XXL + theme::space::M;

/// Posição da versão escolhida na linha do tempo: 0 é a mais antiga (esquerda). A lista vem da
/// mais nova para a mais antiga; sem escolha (ainda carregando) vale a mais nova.
fn indice_na_barra(versoes: &[Linha], escolhida: Option<u64>) -> usize {
    let ultima = versoes.len().saturating_sub(1);
    escolhida
        .and_then(|id| versoes.iter().position(|linha| linha.info.id.0 == id))
        .map_or(ultima, |lugar| ultima - lugar)
}

/// A barra da linha do tempo: o trilho e o preenchimento até a posição aparecem nos dois temas
/// (as cores do `theme::slider` compacto), e a altura de 28 px deixa o dedo acertar.
fn barra_do_tempo(ui: &mut egui::Ui, valor: &mut usize, ultimo: usize) -> egui::Response {
    ui.scope(|ui| {
        let dark = ui.visuals().dark_mode;
        let style = ui.style_mut();
        style.spacing.interact_size.y = 28.0;
        style.spacing.slider_rail_height = 4.0;
        for (visuals, fill, outline) in [
            (&mut style.visuals.widgets.inactive, 66, 170),
            (&mut style.visuals.widgets.hovered, 108, 190),
            (&mut style.visuals.widgets.active, 128, 200),
        ] {
            visuals.bg_fill = egui::Color32::from_gray(if dark { fill } else { 230 - fill / 2 });
            visuals.fg_stroke = egui::Stroke::new(
                1.0,
                egui::Color32::from_gray(if dark { outline } else { 230 - outline }),
            );
            visuals.expansion = 0.0;
        }
        ui.add(
            egui::Slider::new(valor, 0..=ultimo)
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Circle)
                .trailing_fill(true),
        )
    })
    .inner
}

/// O texto de uma linha da lista, sempre em duas linhas (a altura é a mesma para `show_rows`).
/// Sem nome: a hora, e embaixo "origem · mudança". Com nome: o nome em destaque, e embaixo
/// "hora · origem · mudança".
fn texto_da_linha(
    ui: &egui::Ui,
    agora: chrono::DateTime<chrono::Local>,
    linha: &Linha,
) -> egui::WidgetText {
    let info = &linha.info;
    let relativo = rotulo_relativo(agora, info.quando);
    // Na linha com nome a hora vem curta ("08:27", "ontem 09:27"), para a mudança de tamanho caber.
    let hora = relativo
        .rsplit(" · ")
        .next()
        .unwrap_or(&relativo)
        .to_owned();
    let detalhe = format!(
        "{} · {}",
        rotulo_origem(info.origem),
        rotulo_mudanca(linha.delta)
    );
    let Some(nome) = &info.nome else {
        return RichText::new(format!("{relativo}\n{detalhe}"))
            .size(13.0)
            .into();
    };
    let fonte = FontId::proportional(13.0);
    let largura = (ui.available_width() - 2.0 * ui.spacing().button_padding.x - 2.0).max(40.0);
    // O nome ocupa uma linha só: corta com reticências até caber, para a segunda não sumir.
    let mut letras: Vec<char> = nome.chars().collect();
    let mut cortado = false;
    let nome = loop {
        let candidato: String =
            letras.iter().collect::<String>() + if cortado { "…" } else { "" };
        let cabe = ui.ctx().fonts_mut(|fontes| {
            fontes
                .layout_no_wrap(candidato.clone(), fonte.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
                <= largura
        });
        if cabe || letras.len() <= 1 {
            break candidato;
        }
        letras.pop();
        cortado = true;
    };
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: largura,
            max_rows: 2,
            break_anywhere: true,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    job.append(
        &format!("{nome}\n"),
        0.0,
        TextFormat {
            font_id: fonte.clone(),
            color: theme::accent(ui),
            ..Default::default()
        },
    );
    job.append(
        &format!("{hora} · {detalhe}"),
        0.0,
        TextFormat {
            font_id: fonte,
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    ui.ctx().fonts_mut(|fontes| fontes.layout_job(job)).into()
}

/// Da mais nova para a mais antiga, com a mudança de tamanho de cada uma.
fn linhas(ascendente: Vec<VersaoInfo>) -> Vec<Linha> {
    let mut anterior = 0i64;
    let mut linhas: Vec<Linha> = ascendente
        .into_iter()
        .map(|info| {
            let tamanho = info.tamanho as i64;
            let linha = Linha {
                delta: tamanho - anterior,
                info,
            };
            anterior = tamanho;
            linha
        })
        .collect();
    linhas.reverse();
    linhas
}

/// "há 5 min", "há 2 h", "ontem 14:32", "12/11 09:10": a hora relativa que a lista mostra.
pub fn rotulo_relativo(agora: chrono::DateTime<chrono::Local>, quando: SystemTime) -> String {
    let quando: chrono::DateTime<chrono::Local> = quando.into();
    let segundos = (agora - quando).num_seconds();
    let hora = quando.format("%H:%M");
    let hoje = agora.date_naive();
    if segundos < 45 {
        "agora há pouco".into()
    } else if segundos < 3600 {
        format!("há {} min · {hora}", (segundos / 60).max(1))
    } else if segundos < 6 * 3600 {
        format!("há {} h · {hora}", segundos / 3600)
    } else if quando.date_naive() == hoje {
        format!("hoje {hora}")
    } else if hoje.pred_opt() == Some(quando.date_naive()) {
        format!("ontem {hora}")
    } else if quando.format("%Y").to_string() == agora.format("%Y").to_string() {
        quando.format("%d/%m %H:%M").to_string()
    } else {
        quando.format("%d/%m/%Y %H:%M").to_string()
    }
}

pub fn rotulo_origem(origem: Origem) -> &'static str {
    match origem {
        Origem::Editor => "você",
        Origem::Externa => "mudou fora do app",
        Origem::Sync => "sync",
        Origem::Restauracao => "você · restaurada",
    }
}

/// "+120 B", "−2,3 KB", "mesmo tamanho": quanto o texto mudou de tamanho nesta versão.
pub fn rotulo_mudanca(delta: i64) -> String {
    let tamanho = |bytes: u64| {
        if bytes < 1024 {
            format!("{bytes} B")
        } else {
            format!("{:.1} KB", bytes as f64 / 1024.0).replace('.', ",")
        }
    };
    match delta {
        0 => "mesmo tamanho".into(),
        d if d > 0 => format!("+{}", tamanho(d as u64)),
        d => format!("−{}", tamanho(d.unsigned_abs())),
    }
}

/// Fundo do que entrou e do que saiu, nos dois temas.
fn diff_colors(dark: bool) -> (egui::Color32, egui::Color32) {
    if dark {
        (
            egui::Color32::from_rgb(29, 65, 46),
            egui::Color32::from_rgb(70, 33, 35),
        )
    } else {
        (
            egui::Color32::from_rgb(211, 243, 219),
            egui::Color32::from_rgb(255, 219, 219),
        )
    }
}

#[derive(Default)]
pub struct Manager {
    pub open: bool,
    history: Option<Leitor>,
    files: Vec<PathBuf>,
    matcher: Option<rustidian::search::FileMatcher>,
    path: Option<PathBuf>,
    versions: Vec<Linha>,
    selected: Option<u64>,
    text: Option<String>,
    changes: Vec<Trecho>,
    expected: Option<Document>,
    input: String,
    choosing_file: bool,
    show_changes: bool,
    /// Compara a versão com o texto de agora (e não com a versão anterior).
    compare_current: bool,
    loading: Option<Receiver<Result<Loaded, String>>>,
    clearing: Option<Receiver<Result<(), String>>>,
    confirm_clear: bool,
    error: Option<String>,
    notice: Option<String>,
    refresh_due: bool,
    focus: bool,
    selected_file: usize,
    pick_when_ready: bool,
    /// A barra andou enquanto uma leitura rodava: o que chegar é de uma posição velha.
    recarregar_depois: bool,
    /// Só as versões com nome na lista da esquerda (a barra continua sobre todas).
    only_named: bool,
    naming_open: bool,
    name_input: String,
    name_focus: bool,
    /// A nota e a versão que o diálogo está nomeando; o segundo valor diz se tinha nome.
    name_target: Option<(PathBuf, u64, bool)>,
    /// A resposta do pedido de nome; o booleano diz se era para tirar o nome.
    naming: Option<(Receiver<Result<(), String>>, bool)>,
    erase_open: bool,
    erase_input: String,
    erase_all: bool,
    erase_focus: bool,
    erasing: Option<Apagando>,
    /// A barra mexeu na escolha: a lista da esquerda rola até a linha escolhida.
    rolar_lista: bool,
}

impl Manager {
    pub fn changed(&mut self, ctx: &egui::Context) {
        if self.open {
            self.refresh_due = true;
            ctx.request_repaint();
        }
    }
    pub fn active(&self) -> bool {
        self.open
            || self.confirm_clear
            || self.clearing.is_some()
            || self.naming_open
            || self.naming.is_some()
            || self.erase_open
            || self.erasing.is_some()
    }
    pub fn open(
        &mut self,
        history: Leitor,
        vault: &Vault,
        path: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        *self = Self {
            open: true,
            history: Some(history),
            focus: true,
            show_changes: true,
            ..Default::default()
        };
        self.select_file(path, vault, ctx);
    }

    pub fn request_clear(&mut self) {
        self.confirm_clear = true;
    }

    pub fn error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub fn restored(&mut self, document: Document, ctx: &egui::Context) {
        self.expected = Some(document);
        self.error = None;
        self.notice = Some("Versão restaurada".into());
        // A versão nova (origem "restaurada") entra no topo da lista e fica escolhida.
        self.selected = None;
        self.reload(ctx, true);
    }

    /// Escolhe a versão da posição `indice` da linha do tempo (0 = a mais antiga) e mostra o texto
    /// dela. A barra e os botões ◀ ▶ chamam isto a cada passo do arrasto.
    pub(crate) fn ir_para(&mut self, indice: usize, ctx: &egui::Context) {
        let Some(ultimo) = self.versions.len().checked_sub(1) else {
            return;
        };
        let id = self.versions[ultimo - indice.min(ultimo)].info.id.0;
        if self.selected != Some(id) {
            self.rolar_lista = true;
            self.escolher(id, ctx);
        }
    }

    /// Uma leitura por vez: se já há uma em andamento só marca que falta reler, e a última
    /// posição sempre vence (o `poll` descarta o que chegar da posição velha).
    fn escolher(&mut self, id: u64, ctx: &egui::Context) {
        self.selected = Some(id);
        self.error = None;
        if self.loading.is_some() {
            self.recarregar_depois = true;
            ctx.request_repaint();
        } else {
            self.reload_com(ctx, false, false);
        }
    }

    fn pedir_apagar(&mut self, historico: &Historico, trecho: &str) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let pedido = if self.erase_all {
            historico.apagar_texto_em_todas(trecho).map(Apagando::Todas)
        } else {
            historico.apagar_texto(&path, trecho).map(Apagando::Nota)
        };
        match pedido {
            Ok(apagando) => {
                self.error = None;
                self.erasing = Some(apagando);
            }
            Err(error) => self.error = Some(error),
        }
        // O texto colado não fica na memória da tela depois do pedido.
        self.erase_input.clear();
    }

    /// A resposta do pedido de apagar, quando chegou: o aviso e o erro.
    fn terminou_de_apagar(&mut self) -> Option<Result<String, String>> {
        fn cru<T>(receiver: &Receiver<Result<T, String>>) -> Option<Result<T, String>> {
            match receiver.try_recv() {
                Ok(resposta) => Some(resposta),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Não foi possível apagar o trecho do histórico.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            }
        }
        let resposta = match self.erasing.as_ref()? {
            Apagando::Nota(receiver) => cru(receiver).map(|resposta| {
                resposta.map(|versoes| match versoes {
                    0 => "O trecho não estava no histórico".to_string(),
                    n => format!("Apagado de {}", versoes_por_extenso(n)),
                })
            }),
            Apagando::Todas(receiver) => cru(receiver).map(|resposta| {
                resposta.map(|total| match total.versoes {
                    0 => "O trecho não estava no histórico".to_string(),
                    n => format!(
                        "Apagado de {} em {}",
                        versoes_por_extenso(n),
                        match total.notas {
                            1 => "1 nota".to_string(),
                            m => format!("{m} notas"),
                        }
                    ),
                })
            }),
        };
        if resposta.is_some() {
            self.erasing = None;
        }
        resposta
    }

    fn pedir_nome(&mut self, historico: &Historico, nome: Option<String>) {
        let Some((path, id, tinha_nome)) = self.name_target.take() else {
            return;
        };
        self.naming_open = false;
        let tirar = nome.as_ref().is_none_or(|nome| nome.trim().is_empty());
        if tirar && !tinha_nome {
            return;
        }
        match historico.nomear(&path, VersaoId(id), nome) {
            Ok(receiver) => self.naming = Some((receiver, tirar)),
            Err(error) => self.error = Some(error),
        }
    }

    fn select_file(&mut self, path: Option<PathBuf>, vault: &Vault, ctx: &egui::Context) {
        self.error = None;
        self.expected = path.as_ref().and_then(|path| {
            vault.read(path).ok().or_else(|| {
                matches!(vault.root.join(path).try_exists(), Ok(false))
                    .then(|| Document::new(path.clone()))
            })
        });
        self.input = path
            .as_ref()
            .map(|p| rustidian::paths::display_path(p))
            .unwrap_or_default();
        self.path = path;
        self.selected = None;
        self.text = None;
        self.versions.clear();
        self.changes.clear();
        self.selected_file = 0;
        self.choosing_file = self.path.is_none();
        self.show_changes = true;
        self.reload(ctx, true);
    }

    fn reload(&mut self, ctx: &egui::Context, refresh_list: bool) {
        self.reload_com(ctx, refresh_list, true);
    }

    /// `limpar_texto` falso mantém o texto de antes na tela até o novo chegar (a barra arrasta sem
    /// piscar; com a leitura em andamento Copiar e Restaurar ficam desabilitados).
    fn reload_com(&mut self, ctx: &egui::Context, refresh_list: bool, limpar_texto: bool) {
        let Some(history) = self.history.clone() else {
            return;
        };
        let path = self.path.clone();
        let selected = self.selected;
        let current = self.expected.as_ref().map(|document| document.text.clone());
        let compare_current = self.compare_current;
        // Da mais nova para a mais antiga, como a lista da tela.
        let known: Vec<u64> = self.versions.iter().map(|linha| linha.info.id.0).collect();
        let (sender, receiver) = mpsc::channel();
        self.loading = Some(receiver);
        // A failed version query must never leave another version's preview
        // copyable or present it as the newly selected snapshot.
        self.recarregar_depois = false;
        if limpar_texto {
            self.text = None;
            self.changes.clear();
        }
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let files = refresh_list.then(|| history.notas()).transpose()?;
                let versions = if refresh_list {
                    Some(linhas(
                        path.as_ref()
                            .map(|path| history.versoes(path))
                            .transpose()?
                            .unwrap_or_default(),
                    ))
                } else {
                    None
                };
                let order: Vec<u64> = versions.as_ref().map_or(known, |versions| {
                    versions.iter().map(|linha| linha.info.id.0).collect()
                });
                let selected = selected
                    .filter(|id| order.contains(id))
                    .or_else(|| order.first().copied());
                let text = match (&path, selected) {
                    (Some(path), Some(id)) => Some(history.ler(path, VersaoId(id))?),
                    _ => None,
                };
                let changes = match (&path, &text, selected) {
                    (_, Some(text), _) if compare_current => {
                        diff_palavras(text, current.as_deref().unwrap_or_default())
                    }
                    (Some(path), Some(text), Some(id)) => {
                        // O "antes" é a versão anterior; a primeira versão não tem antes.
                        let before = order
                            .iter()
                            .position(|candidate| *candidate == id)
                            .and_then(|at| order.get(at + 1))
                            .map(|previous| history.ler(path, VersaoId(*previous)))
                            .transpose()?
                            .unwrap_or_default();
                        diff_palavras(&before, text)
                    }
                    _ => Vec::new(),
                };
                Ok(Loaded {
                    files,
                    path,
                    versions,
                    selected,
                    text,
                    changes,
                })
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.loading {
            match receiver.try_recv() {
                Ok(Ok(loaded)) => {
                    let velha = std::mem::take(&mut self.recarregar_depois);
                    if let Some(files) = loaded.files {
                        self.matcher = Some(rustidian::search::FileMatcher::new(files.clone()));
                        self.files = files;
                    }
                    self.path = loaded.path;
                    if let Some(versions) = loaded.versions {
                        self.versions = versions;
                    }
                    self.loading = None;
                    if velha {
                        // O texto que chegou é de uma posição que a barra já deixou: a escolha
                        // atual (que a barra mexeu) não pode pular de volta; lê de novo.
                        self.reload_com(ctx, false, false);
                    } else {
                        self.selected = loaded.selected;
                        self.text = loaded.text;
                        self.changes = loaded.changes;
                    }
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.recarregar_depois = false;
                    self.text = None;
                    self.changes.clear();
                    self.loading = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Não foi possível ler o histórico local.".into());
                    self.recarregar_depois = false;
                    self.text = None;
                    self.changes.clear();
                    self.loading = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver) = &self.clearing {
            match receiver.try_recv() {
                Ok(result) => {
                    self.clearing = None;
                    match result {
                        Ok(()) => {
                            self.files.clear();
                            self.matcher = None;
                            self.versions.clear();
                            self.selected = None;
                            self.text = None;
                            self.changes.clear();
                            self.notice = Some("Histórico apagado".into());
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.clearing = None;
                    self.error = Some("Não foi possível apagar o histórico local.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some((receiver, tirou)) = &self.naming {
            let tirou = *tirou;
            let resposta = match receiver.try_recv() {
                Ok(resposta) => Some(resposta),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Não foi possível nomear a versão.".to_string()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(resposta) = resposta {
                self.naming = None;
                match resposta {
                    Ok(()) => {
                        self.notice = Some(if tirou {
                            "Nome tirado".into()
                        } else {
                            "Versão nomeada".into()
                        });
                        // A lista volta com o nome e a versão escolhida continua escolhida.
                        if self.open {
                            self.reload(ctx, true);
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if let Some(resposta) = self.terminou_de_apagar() {
            match resposta {
                Ok(aviso) => {
                    self.notice = Some(aviso);
                    // A lista e o texto voltam sem o trecho.
                    if self.open {
                        self.reload(ctx, true);
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        // Like the reference, keep displayed versions fixed while browsing.
        // File selection/reopening reads new metadata; version clicks read text.
        if self.refresh_due
            && self.choosing_file
            && self.loading.is_none()
            && self.clearing.is_none()
            && !self.confirm_clear
        {
            self.refresh_due = false;
            self.reload(ctx, true);
        }
        if self.loading.is_some()
            || self.clearing.is_some()
            || self.naming.is_some()
            || self.erasing.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        vault: &Vault,
        historico: &Historico,
    ) -> Option<Restore> {
        self.poll(ctx);
        let mut restore = None;
        let mut select_file = None;
        let mut select_version = None;
        let mut compare_changed = false;
        let mut timeline_to = None;
        let mut open_naming = false;
        let mut open_erase = false;
        let touch = egui::vec2(0.0, theme::space::CONTROL);
        let mut close = false;
        if self.open {
            let dialog =
                egui::Modal::new(egui::Id::unique("file-recovery")).frame(theme::dialog_frame(ctx));
            let modal = dialog.show(ctx, |ui| {
                theme::dialog_style(ui);
                ui.set_width(786.0_f32.min((ctx.content_rect().width() - 48.0).max(280.0)));
                let height = (ctx.content_rect().height() - 190.0).clamp(220.0, 580.0);
                // Keep controls stationary while async content and clipboard
                // notices arrive, including the first version's sizing pass.
                ui.set_height(height + 56.0);
                let busy = self.loading.is_some()
                    || self.clearing.is_some()
                    || self.naming.is_some()
                    || self.erasing.is_some();
                let body_size = egui::vec2(ui.available_width(), height);
                let body_layout = Layout::left_to_right(Align::Min);
                ui.allocate_ui_with_layout(body_size, body_layout, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(225.0_f32.min(ui.available_width() * 0.35), height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_min_width(ui.max_rect().width());
                            let input_id = ui.make_persistent_id("recovery-file-input");
                            if self.focus {
                                ui.memory_mut(|memory| memory.request_focus(input_id));
                            }
                            let input = ui.add(
                                TextEdit::singleline(&mut self.input)
                                    .id(input_id)
                                    .return_key(None)
                                    .hint_text("Escolha um arquivo...")
                                    .desired_width(ui.available_width()),
                            );
                            if self.focus {
                                input.request_focus();
                                self.focus = ui.is_sizing_pass();
                            }
                            ui_probe::record(ctx, "recovery-files", input.rect);
                            if input.changed() {
                                self.choosing_file = true;
                                self.selected_file = 0;
                            }
                            ui.add_space(crate::theme::space::S);
                            let com_nome =
                                self.versions.iter().any(|linha| linha.info.nome.is_some());
                            if !com_nome {
                                self.only_named = false;
                            }
                            let mostrando_versoes =
                                !self.choosing_file && !self.versions.is_empty();
                            // O filtro não mexe na barra: ela segue sobre todas as versões.
                            let mut lista_h = height - 50.0;
                            if mostrando_versoes && com_nome {
                                let filtro = ui.add(
                                    egui::Button::selectable(self.only_named, "Só com nome")
                                        .min_size(egui::vec2(ui.available_width(), touch.y)),
                                );
                                ui_probe::record(ctx, "recovery-only-named", filtro.rect);
                                if filtro.clicked() {
                                    self.only_named = !self.only_named;
                                }
                                lista_h -= touch.y + ui.spacing().item_spacing.y;
                            }
                            let mostrar: Vec<usize> = (0..self.versions.len())
                                .filter(|&i| {
                                    !self.only_named || self.versions[i].info.nome.is_some()
                                })
                                .collect();
                            let scroll = ScrollArea::vertical()
                                .id_salt(("recovery-versions", &self.path, self.choosing_file))
                                .auto_shrink([false, false])
                                .max_height(lista_h)
                                .min_scrolled_height(lista_h);
                            if mostrando_versoes {
                                let now = chrono::Local::now();
                                scroll.show_rows(ui, LINHA, mostrar.len(), |ui, rows| {
                                    if std::mem::take(&mut self.rolar_lista)
                                        && let Some(row) = mostrar.iter().position(|&i| {
                                            Some(self.versions[i].info.id.0) == self.selected
                                        })
                                    {
                                        // As linhas fora de `rows` não existem na tela: o retângulo
                                        // é calculado pelo índice, e a lista rola até ele.
                                        let passo = LINHA + ui.spacing().item_spacing.y;
                                        let topo = ui.max_rect().top()
                                            + (row as f32 - rows.start as f32) * passo;
                                        let alvo = egui::Rect::from_min_size(
                                            egui::pos2(ui.max_rect().left(), topo),
                                            egui::vec2(ui.max_rect().width(), LINHA),
                                        );
                                        let alinhar =
                                            (!rows.contains(&row)).then_some(Align::Center);
                                        ui.scroll_to_rect(alvo, alinhar);
                                    }
                                    for &i in &mostrar[rows] {
                                        let linha = &self.versions[i];
                                        let version = &linha.info;
                                        let button = ui.add_enabled(
                                            !busy,
                                            egui::Button::selectable(
                                                self.selected == Some(version.id.0),
                                                texto_da_linha(ui, now, linha),
                                            )
                                            .min_size(egui::vec2(ui.available_width(), LINHA))
                                            .wrap(),
                                        );
                                        ui_probe::record(
                                            ctx,
                                            format!("recovery-version:{}", version.id.0),
                                            button.rect.intersect(ui.clip_rect()),
                                        );
                                        if button.clicked() {
                                            select_version = Some(version.id.0);
                                        }
                                    }
                                });
                            } else {
                                scroll.show(ui, |ui| {
                                    if self.choosing_file {
                                        let matches = self
                                            .matcher
                                            .as_mut()
                                            .map(|matcher| matcher.rank(&self.input, 100))
                                            .unwrap_or_default();
                                        let mut moved = false;
                                        if ui.input_mut(|i| {
                                            i.consume_key(
                                                egui::Modifiers::NONE,
                                                egui::Key::ArrowDown,
                                            )
                                        }) {
                                            self.selected_file = (self.selected_file + 1)
                                                .min(matches.len().saturating_sub(1));
                                            moved = true;
                                        }
                                        if ui.input_mut(|i| {
                                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                                        }) {
                                            self.selected_file =
                                                self.selected_file.saturating_sub(1);
                                            moved = true;
                                        }
                                        self.selected_file =
                                            self.selected_file.min(matches.len().saturating_sub(1));
                                        if ui.input_mut(|i| {
                                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                        }) {
                                            self.pick_when_ready = true;
                                        }
                                        if self.pick_when_ready && !busy {
                                            self.pick_when_ready = false;
                                            select_file = matches.get(self.selected_file).cloned();
                                        }
                                        for (index, path) in matches.iter().enumerate() {
                                            let button = ui.add_enabled(
                                                !busy,
                                                egui::Button::selectable(
                                                    index == self.selected_file,
                                                    path.display().to_string(),
                                                )
                                                .min_size(egui::vec2(
                                                    ui.available_width(),
                                                    crate::theme::space::CONTROL,
                                                ))
                                                .wrap(),
                                            );
                                            ui_probe::record(
                                                ctx,
                                                format!("recovery-file:{}", path.display()),
                                                button.rect.intersect(ui.clip_rect()),
                                            );
                                            if button.clicked() {
                                                select_file = Some(path.clone());
                                            }
                                            if index == self.selected_file
                                                && (moved || input.changed())
                                            {
                                                button.scroll_to_me(Some(Align::Center));
                                            }
                                        }
                                    }
                                    if self.files.is_empty() && !busy {
                                        ui.label("Nenhuma versão encontrada.");
                                    }
                                });
                            }
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let name = self
                                .path
                                .as_ref()
                                .and_then(|path| path.file_stem())
                                .map(|name| name.to_string_lossy())
                                .unwrap_or_default();
                            ui.add_sized(
                                egui::vec2((ui.available_width() - 360.0).max(20.0), 24.0),
                                egui::Label::new(RichText::new(name.as_ref()).strong()).truncate(),
                            )
                            .on_hover_text(name.as_ref());
                            if busy {
                                ui.spinner();
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let button = theme::icon_button(ui, theme::Icon::Close, "Close");
                                ui_probe::record(ctx, "recovery-close", button.rect);
                                close = button.clicked();
                                let button = ui.add_enabled(
                                    !busy && self.text.is_some() && self.expected.is_some(),
                                    egui::Button::new("Restaurar").min_size(touch),
                                );
                                ui_probe::record(ctx, "recovery-restore", button.rect);
                                if button.clicked()
                                    && let (Some(expected), Some(text)) =
                                        (&self.expected, &self.text)
                                {
                                    restore = Some(Restore {
                                        expected: expected.clone(),
                                        text: text.clone(),
                                    });
                                }
                                let button = ui.add_enabled(
                                    !busy && self.text.is_some(),
                                    egui::Button::new("Copiar").min_size(touch),
                                );
                                ui_probe::record(ctx, "recovery-copy", button.rect);
                                if !busy {
                                    ui_probe::record(ctx, "recovery-loaded", button.rect);
                                }
                                if !busy && self.text.is_some() {
                                    ui_probe::record(ctx, "recovery-ready", button.rect);
                                    if let Some(version) = self.selected {
                                        ui_probe::record(
                                            ctx,
                                            format!("recovery-selected:{version}"),
                                            button.rect,
                                        );
                                    }
                                }
                                if button.clicked()
                                    && let Some(text) = &self.text
                                {
                                    ctx.copy_text(text.clone());
                                    self.notice =
                                        Some("Copiado para a área de transferência".into());
                                }
                                let nome_atual = self
                                    .selected
                                    .and_then(|id| self.versions.iter().find(|l| l.info.id.0 == id))
                                    .map(|linha| linha.info.nome.is_some());
                                let button = ui.add_enabled(
                                    !busy && nome_atual.is_some(),
                                    egui::Button::new(if nome_atual == Some(true) {
                                        "Mudar nome"
                                    } else {
                                        "Dar nome"
                                    })
                                    .min_size(touch),
                                );
                                ui_probe::record(ctx, "recovery-name", button.rect);
                                open_naming = button.clicked();
                            });
                        });
                        // Alvos de 32 px ou mais: o dedo no Android acerta.
                        ui.horizontal_wrapped(|ui| {
                            let ready = !busy && self.text.is_some();
                            let changes = ui.add_enabled(
                                ready,
                                egui::Button::selectable(self.show_changes, "Mostrar alterações")
                                    .min_size(touch),
                            );
                            ui_probe::record(ctx, "recovery-changes", changes.rect);
                            if changes.clicked() {
                                self.show_changes = !self.show_changes;
                            }
                            let current = ui.add_enabled(
                                ready && self.show_changes,
                                egui::Button::selectable(
                                    self.compare_current,
                                    "Comparar com o texto atual",
                                )
                                .min_size(touch),
                            );
                            ui_probe::record(ctx, "recovery-compare", current.rect);
                            if current.clicked() {
                                self.compare_current = !self.compare_current;
                                compare_changed = true;
                            }
                            let apagar = ui.add_enabled(
                                !busy && self.path.is_some() && !self.choosing_file,
                                egui::Button::new("Apagar do histórico…").min_size(touch),
                            );
                            ui_probe::record(ctx, "recovery-erase", apagar.rect);
                            open_erase = apagar.clicked();
                        });
                        let total = self.versions.len();
                        if !self.choosing_file && total >= 2 {
                            // A barra e os botões nunca ficam desabilitados com a leitura em
                            // andamento: desabilitar no meio mataria o arrasto.
                            let atual = indice_na_barra(&self.versions, self.selected);
                            let mut novo = atual;
                            ui.horizontal(|ui| {
                                let sp = ui.spacing().item_spacing.x;
                                let anterior = ui
                                    .add_enabled_ui(atual > 0, |ui| {
                                        theme::icon_button(ui, theme::Icon::Back, "Versão anterior")
                                    })
                                    .inner;
                                ui_probe::record(ctx, "recovery-timeline-prev", anterior.rect);
                                if anterior.clicked() {
                                    novo = atual.saturating_sub(1);
                                }
                                ui.spacing_mut().slider_width =
                                    (ui.available_width() - theme::space::CONTROL - sp).max(60.0);
                                let mut valor = atual;
                                let barra = barra_do_tempo(ui, &mut valor, total - 1);
                                barra.widget_info(|| {
                                    egui::WidgetInfo::slider(true, valor as f64, "Linha do tempo")
                                });
                                ui_probe::record(ctx, "recovery-timeline", barra.rect);
                                if valor != atual {
                                    novo = valor;
                                }
                                // Um tracinho sob o trilho marca cada versão com nome.
                                let raio = barra.rect.height() / 2.5;
                                let (de, ate) =
                                    (barra.rect.left() + raio, barra.rect.right() - raio);
                                let marca = theme::accent(ui);
                                for (lugar, linha) in self.versions.iter().enumerate() {
                                    if linha.info.nome.is_some() {
                                        let x = de
                                            + (ate - de) * (total - 1 - lugar) as f32
                                                / (total - 1) as f32;
                                        ui.painter().line_segment(
                                            [
                                                egui::pos2(x, barra.rect.bottom() - 7.0),
                                                egui::pos2(x, barra.rect.bottom() + 1.0),
                                            ],
                                            egui::Stroke::new(2.0, marca),
                                        );
                                    }
                                }
                                let proxima = ui
                                    .add_enabled_ui(atual + 1 < total, |ui| {
                                        theme::icon_button(
                                            ui,
                                            theme::Icon::Forward,
                                            "Próxima versão",
                                        )
                                    })
                                    .inner;
                                ui_probe::record(ctx, "recovery-timeline-next", proxima.rect);
                                if proxima.clicked() {
                                    novo = (atual + 1).min(total - 1);
                                }
                            });
                            if let Some(linha) = self.versions.get(total - 1 - atual) {
                                let mut texto = LayoutJob::default();
                                let fraco = TextFormat {
                                    font_id: FontId::proportional(13.0),
                                    color: ui.visuals().weak_text_color(),
                                    ..Default::default()
                                };
                                if let Some(nome) = &linha.info.nome {
                                    texto.append(
                                        &format!("{nome} · "),
                                        0.0,
                                        TextFormat {
                                            color: theme::accent(ui),
                                            ..fraco.clone()
                                        },
                                    );
                                }
                                texto.append(
                                    &format!(
                                        "{} · {}",
                                        rotulo_relativo(chrono::Local::now(), linha.info.quando),
                                        rotulo_origem(linha.info.origem)
                                    ),
                                    0.0,
                                    fraco,
                                );
                                ui.add(egui::Label::new(texto).truncate());
                            }
                            if novo != atual {
                                timeline_to = Some(novo);
                            }
                        }
                        if self.show_changes && self.text.is_some() {
                            let caption = if self.compare_current {
                                "Desta versão até agora. "
                            } else {
                                "O que mudou nesta versão. "
                            };
                            let (green, red) = diff_colors(ui.visuals().dark_mode);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = theme::space::XS;
                                ui.label(RichText::new(caption).small().weak());
                                ui.label(RichText::new(" entrou ").small().background_color(green));
                                ui.label(RichText::new(" saiu ").small().background_color(red));
                            });
                        }
                        ui.separator();
                        // O que sobra da coluna: a barra do tempo e a segunda fileira de
                        // botões (quando quebra) tiram altura do texto, e não da janela.
                        let resto = ui.available_height().max(60.0);
                        ScrollArea::both()
                            .id_salt(("recovery-text", self.selected, self.compare_current))
                            .auto_shrink([false, false])
                            .max_height(resto)
                            .min_scrolled_height(resto)
                            .show(ui, |ui| {
                                let response = if self.show_changes {
                                    let (green, red) = diff_colors(ui.visuals().dark_mode);
                                    let color = ui.visuals().text_color();
                                    let mut job = egui::text::LayoutJob::default();
                                    for part in &self.changes {
                                        let (background, strikethrough) = match part.tipo {
                                            Tipo::Igual => {
                                                (egui::Color32::TRANSPARENT, egui::Stroke::NONE)
                                            }
                                            Tipo::Saiu => (red, egui::Stroke::new(1.0, color)),
                                            Tipo::Entrou => (green, egui::Stroke::NONE),
                                        };
                                        job.append(
                                            &part.texto,
                                            0.0,
                                            egui::TextFormat {
                                                font_id: FontId::proportional(18.0),
                                                color,
                                                background,
                                                strikethrough,
                                                ..Default::default()
                                            },
                                        );
                                    }
                                    ui.label(job)
                                } else {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(self.text.as_deref().unwrap_or_default())
                                                .size(18.0),
                                        )
                                        .selectable(true)
                                        .wrap(),
                                    )
                                };
                                ui_probe::record(
                                    ctx,
                                    "recovery-text",
                                    response.rect.intersect(ui.clip_rect()),
                                );
                            });
                    });
                });
                ScrollArea::vertical()
                    .id_salt("recovery-status")
                    .max_height(48.0)
                    .min_scrolled_height(48.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(error) = &self.error {
                            let response = ui.colored_label(ui.visuals().error_fg_color, error);
                            ui_probe::record(
                                ctx,
                                "recovery-error",
                                response.rect.intersect(ui.clip_rect()),
                            );
                        } else if let Some(notice) = &self.notice {
                            ui.label(notice);
                        }
                    });
            });
            ui_probe::record(ctx, "recovery-dialog", modal.response.rect);
            if (close || modal.should_close())
                && !self.confirm_clear
                && !self.naming_open
                && !self.erase_open
            {
                self.open = false;
                self.loading = None;
                self.refresh_due = false;
            }
        }
        if self.confirm_clear {
            let mut cancel = false;
            let mut clear = false;
            let dialog = egui::Modal::new(egui::Id::unique("clear-local-history"))
                .frame(theme::dialog_frame(ctx));
            let modal = dialog.show(ctx, |ui| {
                theme::dialog_style(ui);
                ui.set_width(390.0);
                cancel |=
                    theme::dialog_header(ui, "Apagar o histórico?", "Fechar diálogo").clicked();
                ui.label("Isto apagará de vez todas as versões locais deste cofre.");
                let (confirm, dismiss) = theme::dialog_actions(ui, "Limpar", true);
                ui_probe::record(ctx, "recovery-clear-cancel", dismiss.rect);
                ui_probe::record(ctx, "recovery-clear-confirm", confirm.rect);
                cancel |= dismiss.clicked();
                clear = confirm.clicked();
            });
            if cancel || modal.should_close() {
                self.confirm_clear = false;
            }
            if clear {
                self.confirm_clear = false;
                self.loading = None;
                match historico.limpar() {
                    Ok(receiver) => self.clearing = Some(receiver),
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if open_naming && let (Some(path), Some(id)) = (self.path.clone(), self.selected) {
            let atual = self
                .versions
                .iter()
                .find(|linha| linha.info.id.0 == id)
                .and_then(|linha| linha.info.nome.clone());
            self.name_target = Some((path, id, atual.is_some()));
            self.name_input = atual.unwrap_or_default();
            self.name_focus = true;
            self.naming_open = true;
        }
        if self.naming_open {
            let tinha_nome = self
                .name_target
                .as_ref()
                .is_some_and(|(_, _, tinha)| *tinha);
            let (mut save, mut remove, mut cancel) = (false, false, false);
            let dialog =
                egui::Modal::new(egui::Id::unique("name-version")).frame(theme::dialog_frame(ctx));
            let modal = dialog.show(ctx, |ui| {
                theme::dialog_style(ui);
                ui.set_width(390.0);
                cancel |= theme::dialog_header(ui, "Nome da versão", "Fechar diálogo").clicked();
                let field_id = ui.make_persistent_id("recovery-name-input");
                let field = ui.add(
                    TextEdit::singleline(&mut self.name_input)
                        .id(field_id)
                        .char_limit(80)
                        .hint_text("Ex.: Antes de reescrever")
                        .desired_width(ui.available_width()),
                );
                if self.name_focus {
                    field.request_focus();
                    self.name_focus = ui.is_sizing_pass();
                }
                ui_probe::record(ctx, "recovery-name-field", field.rect);
                // Enter só vale quando o campo tinha o foco: nunca antes de ele existir.
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.label(
                    RichText::new("Versões com nome nunca saem do histórico.")
                        .small()
                        .weak(),
                );
                if tinha_nome {
                    let button = ui.add(egui::Button::new("Tirar o nome").min_size(touch));
                    ui_probe::record(ctx, "recovery-name-remove", button.rect);
                    remove = button.clicked();
                }
                let (confirm, dismiss) = theme::dialog_actions(ui, "Salvar", false);
                ui_probe::record(ctx, "recovery-name-save", confirm.rect);
                ui_probe::record(ctx, "recovery-name-cancel", dismiss.rect);
                save |= confirm.clicked();
                cancel |= dismiss.clicked();
            });
            if save {
                let nome = self.name_input.clone();
                self.pedir_nome(historico, Some(nome));
            } else if remove {
                self.pedir_nome(historico, None);
            } else if cancel || modal.should_close() {
                self.naming_open = false;
                self.name_target = None;
            }
        }
        if open_erase {
            self.erase_input.clear();
            self.erase_all = false;
            self.erase_focus = true;
            self.erase_open = true;
        }
        if self.erase_open {
            let (mut apagar, mut cancel) = (false, false);
            let trecho = self.erase_input.clone();
            // O trecho vai exato (sem trim) para o fundo; só vale com 3 letras de verdade.
            let valido = trecho.chars().filter(|c| !c.is_whitespace()).count() >= 3;
            let ainda_na_nota = valido
                && self
                    .expected
                    .as_ref()
                    .is_some_and(|document| document.text.contains(&trecho));
            let dialog = egui::Modal::new(egui::Id::unique("erase-history-text"))
                .frame(theme::dialog_frame(ctx));
            let modal = dialog.show(ctx, |ui| {
                theme::dialog_style(ui);
                ui.set_width(440.0_f32.min((ctx.content_rect().width() - 48.0).max(280.0)));
                cancel |=
                    theme::dialog_header(ui, "Apagar um trecho do histórico", "Fechar diálogo")
                        .clicked();
                ui.label(
                    "O trecho some de todas as versões desta nota. Serve para uma senha ou um \
                     dado colado por engano. Não dá para desfazer.",
                );
                let field = ui.add(
                    TextEdit::multiline(&mut self.erase_input)
                        .id(ui.make_persistent_id("recovery-erase-input"))
                        .desired_rows(3)
                        .hint_text("Cole aqui o trecho a apagar")
                        .desired_width(ui.available_width()),
                );
                if self.erase_focus {
                    field.request_focus();
                    self.erase_focus = ui.is_sizing_pass();
                }
                ui_probe::record(ctx, "recovery-erase-field", field.rect);
                // Um botão alternável como os da tela: o quadradinho do checkbox some no tema escuro.
                let all = ui.add(
                    egui::Button::selectable(self.erase_all, "Em todas as notas do cofre")
                        .min_size(touch),
                );
                ui_probe::record(ctx, "recovery-erase-all", all.rect);
                if all.clicked() {
                    self.erase_all = !self.erase_all;
                }
                if ainda_na_nota {
                    let aviso = ui.colored_label(
                        ui.visuals().error_fg_color,
                        "A nota ainda tem este trecho. Tire da nota também, senão a próxima \
                         versão o traz de volta.",
                    );
                    ui_probe::record(ctx, "recovery-erase-warning", aviso.rect);
                }
                let (confirm, dismiss) = theme::dialog_actions_enabled(ui, "Apagar", true, valido);
                ui_probe::record(ctx, "recovery-erase-confirm", confirm.rect);
                ui_probe::record(ctx, "recovery-erase-cancel", dismiss.rect);
                apagar = confirm.clicked();
                cancel |= dismiss.clicked();
            });
            if apagar && valido {
                self.erase_open = false;
                self.pedir_apagar(historico, &trecho);
            } else if cancel || modal.should_close() {
                self.erase_open = false;
            }
        }
        if let Some(path) = select_file {
            self.select_file(Some(path), vault, ctx);
        }
        if let Some(indice) = timeline_to {
            self.ir_para(indice, ctx);
        } else if let Some(id) = select_version {
            self.escolher(id, ctx);
        } else if compare_changed {
            self.reload(ctx, false);
        }
        restore
    }
}

/// Acessores só para os testes de GUI (`src/app/gui_tests/historico.rs`): o estado da tela sem
/// passar pelos campos privados.
#[cfg(test)]
impl Manager {
    pub(crate) fn escolhida(&self) -> Option<u64> {
        self.selected
    }

    pub(crate) fn texto_mostrado(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Há leitura, apagamento ou pedido de nome em andamento.
    pub(crate) fn ocupado(&self) -> bool {
        self.loading.is_some()
            || self.clearing.is_some()
            || self.naming.is_some()
            || self.erasing.is_some()
    }

    pub(crate) fn releitura_pendente(&self) -> bool {
        self.recarregar_depois
    }

    /// Finge uma leitura que não termina enquanto o valor devolvido estiver vivo.
    pub(crate) fn simular_leitura(&mut self) -> Box<dyn std::any::Any> {
        let (sender, receiver) = mpsc::channel::<Result<Loaded, String>>();
        self.loading = Some(receiver);
        Box::new(sender)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use chrono::TimeZone;

    fn local(
        ano: i32,
        mes: u32,
        dia: u32,
        hora: u32,
        minuto: u32,
    ) -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(ano, mes, dia, hora, minuto, 0)
            .unwrap()
    }

    #[test]
    fn a_hora_relativa_fala_a_lingua_de_quem_escreve() {
        let agora = local(2026, 11, 18, 15, 30);
        let antes =
            |minutos: i64| -> SystemTime { (agora - chrono::Duration::minutes(minutos)).into() };
        assert_eq!(rotulo_relativo(agora, antes(0)), "agora há pouco");
        assert_eq!(rotulo_relativo(agora, antes(5)), "há 5 min · 15:25");
        assert_eq!(rotulo_relativo(agora, antes(125)), "há 2 h · 13:25");
        // Mais de 6 h atrás, mas hoje.
        let cedo: SystemTime = local(2026, 11, 18, 7, 5).into();
        assert_eq!(rotulo_relativo(agora, cedo), "hoje 07:05");
        let ontem: SystemTime = local(2026, 11, 17, 23, 40).into();
        assert_eq!(rotulo_relativo(agora, ontem), "ontem 23:40");
        let semana: SystemTime = local(2026, 11, 12, 9, 10).into();
        assert_eq!(rotulo_relativo(agora, semana), "12/11 09:10");
        let ano: SystemTime = local(2025, 3, 2, 18, 0).into();
        assert_eq!(rotulo_relativo(agora, ano), "02/03/2025 18:00");
    }

    #[test]
    fn a_mudanca_de_tamanho_aparece_com_sinal() {
        assert_eq!(rotulo_mudanca(0), "mesmo tamanho");
        assert_eq!(rotulo_mudanca(120), "+120 B");
        assert_eq!(rotulo_mudanca(-35), "−35 B");
        assert_eq!(rotulo_mudanca(2350), "+2,3 KB");
        assert_eq!(rotulo_mudanca(-2048), "−2,0 KB");
    }

    #[test]
    fn a_origem_tem_palavras_de_gente() {
        assert_eq!(rotulo_origem(Origem::Editor), "você");
        assert_eq!(rotulo_origem(Origem::Externa), "mudou fora do app");
        assert_eq!(rotulo_origem(Origem::Sync), "sync");
    }
}
