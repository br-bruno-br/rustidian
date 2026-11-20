//! Telas da H3 e da H4: "Galhos do desfazer", "Achar o que apaguei" e "Histórico deste trecho".
//!
//! As três usam o mesmo diálogo. Nada aqui toca o banco: cada pedido vai à linha de fundo do
//! histórico (`rustidian::historico_fundo`) e a resposta chega num `Receiver`, lido a cada quadro.
use crate::{recovery_ui::rotulo_relativo, theme, ui_probe};
use eframe::egui::{self, Align, Layout, RichText, ScrollArea, TextEdit};
use rustidian::historico_fundo::{
    AberturaDesfazer, Apagado, Galhos, Historico, MudancaDoTrecho, NoId, filtrar_apagados,
};
use std::{
    ops::Range,
    path::PathBuf,
    sync::mpsc::{Receiver, TryRecvError},
    time::SystemTime,
};

type Resposta<T> = Receiver<Result<T, String>>;

/// Quantos trechos apagados aparecem de uma vez (a busca estreita o resto).
const LINHAS_MAXIMAS: usize = 60;
/// Quantos caracteres de cada trecho a lista mostra.
const TRECHO_MAXIMO: usize = 420;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum Vista {
    #[default]
    Galhos,
    Apagados,
    Trecho,
}

/// O que o diálogo pede ao app.
pub(crate) enum Acao {
    /// A nota passa a ter o texto do galho escolhido.
    IrPara { nota: PathBuf, texto: String },
    /// Insere o texto onde está o cursor.
    Reinserir(String),
    /// Troca a seleção (faixa em caracteres) por `texto`, se ela ainda é `esperado`.
    TrocarSelecao {
        faixa: Range<usize>,
        esperado: String,
        texto: String,
    },
}

#[derive(Default)]
pub(crate) struct Manager {
    pub open: bool,
    vista: Vista,
    nota: PathBuf,
    galhos: Option<Galhos>,
    apagados: Vec<Apagado>,
    trecho: Vec<MudancaDoTrecho>,
    carregado: bool,
    esperando_galhos: Option<Resposta<Galhos>>,
    esperando_apagados: Option<Resposta<Vec<Apagado>>>,
    esperando_trecho: Option<Resposta<Vec<MudancaDoTrecho>>>,
    indo: Option<Resposta<(String, AberturaDesfazer)>>,
    /// O galho que a pessoa pediu e o app ainda não mandou à linha de fundo.
    pedido: Option<NoId>,
    busca: String,
    foco: bool,
    erro: Option<String>,
    aviso: Option<String>,
    /// A seleção que o histórico do trecho investiga: a faixa em caracteres e o texto.
    selecao: Option<(Range<usize>, String)>,
}

fn espera<T>(rx: &mut Option<Resposta<T>>) -> Option<Result<T, String>> {
    let r = rx.as_ref()?;
    match r.try_recv() {
        Ok(v) => {
            *rx = None;
            Some(v)
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            *rx = None;
            Some(Err("O histórico local parou antes de responder.".into()))
        }
    }
}

fn cortar(texto: &str) -> String {
    let n = texto.chars().count();
    if n <= TRECHO_MAXIMO {
        texto.to_owned()
    } else {
        let mut s: String = texto.chars().take(TRECHO_MAXIMO).collect();
        s.push('…');
        s
    }
}

fn quando(agora: chrono::DateTime<chrono::Local>, t: SystemTime) -> String {
    rotulo_relativo(agora, t)
}

/// "1 trecho" ou "5 trechos".
fn trechos_por_extenso(n: usize) -> String {
    match n {
        1 => "1 trecho".into(),
        n => format!("{n} trechos"),
    }
}

impl Manager {
    pub fn active(&self) -> bool {
        self.open
    }

    fn comecar(&mut self, vista: Vista, nota: PathBuf) {
        *self = Self {
            open: true,
            vista,
            nota,
            foco: true,
            ..Default::default()
        };
    }

    /// Abre "Galhos do desfazer" para a nota.
    pub fn abrir_galhos(&mut self, historico: &Historico, nota: PathBuf) {
        self.comecar(Vista::Galhos, nota);
        match historico.galhos(&self.nota) {
            Ok(rx) => self.esperando_galhos = Some(rx),
            Err(e) => self.erro = Some(e),
        }
    }

    /// Abre "Achar o que apaguei" com o texto que a nota tem agora.
    pub fn abrir_apagados(&mut self, historico: &Historico, nota: PathBuf, atual: &str) {
        self.comecar(Vista::Apagados, nota);
        match historico.achar_apagados(&self.nota, atual) {
            Ok(rx) => self.esperando_apagados = Some(rx),
            Err(e) => self.erro = Some(e),
        }
    }

    /// Abre "Histórico deste trecho" para a faixa `faixa` (em caracteres) do texto `atual`.
    pub fn abrir_trecho(
        &mut self,
        historico: &Historico,
        nota: PathBuf,
        atual: &str,
        faixa: Range<usize>,
    ) {
        self.comecar(Vista::Trecho, nota);
        let byte = |c: usize| atual.char_indices().nth(c).map_or(atual.len(), |(b, _)| b);
        let bytes = byte(faixa.start)..byte(faixa.end);
        self.selecao = Some((faixa, atual[bytes.clone()].to_owned()));
        match historico.historico_do_trecho(&self.nota, atual, bytes) {
            Ok(rx) => self.esperando_trecho = Some(rx),
            Err(e) => self.erro = Some(e),
        }
    }

    /// Mostra um erro no diálogo (o app usa quando a ação pedida não pôde ser feita).
    pub fn erro(&mut self, erro: String) {
        self.erro = Some(erro);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(r) = espera(&mut self.esperando_galhos) {
            match r {
                Ok(g) => self.galhos = Some(g),
                Err(e) => self.erro = Some(e),
            }
            self.carregado = true;
            ctx.request_repaint();
        }
        if let Some(r) = espera(&mut self.esperando_apagados) {
            match r {
                Ok(a) => self.apagados = a,
                Err(e) => self.erro = Some(e),
            }
            self.carregado = true;
            ctx.request_repaint();
        }
        if let Some(r) = espera(&mut self.esperando_trecho) {
            match r {
                Ok(t) => self.trecho = t,
                Err(e) => self.erro = Some(e),
            }
            self.carregado = true;
            ctx.request_repaint();
        }
    }

    fn ocupado(&self) -> bool {
        self.esperando_galhos.is_some()
            || self.esperando_apagados.is_some()
            || self.esperando_trecho.is_some()
            || self.indo.is_some()
    }

    /// Desenha o diálogo (se aberto) e devolve o que o usuário pediu.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Acao> {
        if !self.open {
            return None;
        }
        self.poll(ctx);
        let mut acao = None;
        // Quem foi para um galho: a resposta chega e o texto vai ao app.
        if let Some(r) = espera(&mut self.indo) {
            match r {
                Ok((texto, _)) => {
                    self.open = false;
                    return Some(Acao::IrPara {
                        nota: self.nota.clone(),
                        texto,
                    });
                }
                Err(e) => self.erro = Some(e),
            }
        }
        let titulo = match self.vista {
            Vista::Galhos => "Galhos do desfazer",
            Vista::Apagados => "Achar o que apaguei",
            Vista::Trecho => "Histórico deste trecho",
        };
        let mut fechar = false;
        let dialogo =
            egui::Modal::new(egui::Id::unique("escritor-dialogo")).frame(theme::dialog_frame(ctx));
        let modal = dialogo.show(ctx, |ui| {
            theme::dialog_style(ui);
            ui.set_width(660.0_f32.min((ctx.content_rect().width() - 48.0).max(280.0)));
            fechar |= theme::dialog_header(ui, titulo, "Fechar diálogo").clicked();
            match self.vista {
                Vista::Galhos => self.corpo_galhos(ctx, ui),
                Vista::Apagados => acao = self.corpo_apagados(ctx, ui),
                Vista::Trecho => acao = self.corpo_trecho(ctx, ui),
            }
            if let Some(erro) = &self.erro {
                ui.colored_label(ui.visuals().error_fg_color, erro);
            }
            if let Some(aviso) = &self.aviso {
                ui.label(RichText::new(aviso).small().weak());
            }
        });
        if fechar || modal.should_close() || acao.is_some() {
            self.open = false;
        }
        acao
    }

    fn corpo_galhos(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.label(
            "O Ctrl+Z fica guardado mesmo depois de fechar a nota. Se você desfaz e digita outra \
             coisa, o caminho antigo vira um galho: está aqui, e dá para voltar a ele.",
        );
        let altura = (ctx.content_rect().height() - 260.0).clamp(160.0, 460.0);
        if !self.carregado {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Lendo os galhos…");
            });
            return;
        }
        let Some(galhos) = self.galhos.clone() else {
            return;
        };
        if galhos.galhos.is_empty() {
            ui.label("Esta nota ainda não tem passos de desfazer guardados.");
        } else {
            let n = galhos.galhos.len();
            ui.label(
                RichText::new(format!(
                    "{} · {} no total",
                    if n == 1 {
                        "1 galho".to_owned()
                    } else {
                        format!("{n} galhos")
                    },
                    if galhos.passos == 1 {
                        "1 passo".to_owned()
                    } else {
                        format!("{} passos", galhos.passos)
                    }
                ))
                .small()
                .weak(),
            );
        }
        let agora = chrono::Local::now();
        let ocupado = self.ocupado();
        let mut ir = None;
        ScrollArea::vertical()
            .id_salt("escritor-galhos")
            .max_height(altura)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for galho in &galhos.galhos {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2((ui.available_width() - 150.0).max(120.0), 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                let marca = if galho.atual {
                                    "Você está aqui · "
                                } else if galho.refazer {
                                    "Dá para refazer · "
                                } else {
                                    ""
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{marca}{} · {} passos",
                                        quando(agora, galho.quando),
                                        galho.passos
                                    ))
                                    .small()
                                    .weak(),
                                );
                                ui.add(egui::Label::new(cortar(&galho.trecho)).wrap());
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let botao = ui.add_enabled(
                                !galho.atual && !ocupado,
                                egui::Button::new("Ir para este galho"),
                            );
                            ui_probe::record(ctx, format!("escritor-galho:{}", galho.no.0), botao.rect);
                            if botao.clicked() {
                                ir = Some(galho.no);
                            }
                        });
                    });
                    ui.separator();
                }
                if !galhos.arquivadas.is_empty() {
                    ui.add_space(theme::space::S);
                    ui.label(
                        RichText::new(
                            "Desfazer de antes de uma mudança de fora do aplicativo (só histórico):",
                        )
                        .small()
                        .weak(),
                    );
                    for a in &galhos.arquivadas {
                        ui.label(
                            RichText::new(format!(
                                "{} · {} passos",
                                quando(agora, a.arquivada_em),
                                a.passos
                            ))
                            .small(),
                        );
                    }
                }
            });
        if let Some(no) = ir {
            self.pedir_galho(ctx, no);
        }
    }

    /// O pedido vai ao app, que tem o `Historico`; aqui só marca que está esperando.
    fn pedir_galho(&mut self, ctx: &egui::Context, no: NoId) {
        self.aviso = None;
        self.pedido = Some(no);
        ctx.request_repaint();
    }

    /// O app chama quando viu o pedido de ir para um galho (`galho_pedido`).
    pub fn esperar_galho(&mut self, rx: Resposta<(String, AberturaDesfazer)>) {
        self.indo = Some(rx);
    }

    /// O galho que o usuário acabou de pedir, uma vez.
    pub fn galho_pedido(&mut self) -> Option<(PathBuf, NoId)> {
        let no = self.pedido.take()?;
        Some((self.nota.clone(), no))
    }

    fn corpo_apagados(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> Option<Acao> {
        ui.label(
            "Trechos que já estiveram nesta nota, em versões anteriores, e não estão no texto de \
             agora. Escolha um para colocar de volta.",
        );
        let campo = ui.add(
            TextEdit::singleline(&mut self.busca)
                .id(ui.make_persistent_id("escritor-busca"))
                .hint_text("Buscar uma palavra entre o que você apagou")
                .desired_width(ui.available_width()),
        );
        if self.foco {
            campo.request_focus();
            self.foco = ui.is_sizing_pass();
        }
        ui_probe::record(ctx, "escritor-busca", campo.rect);
        if !self.carregado {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Procurando nas versões…");
            });
            return None;
        }
        let indices = filtrar_apagados(&self.apagados, &self.busca);
        if self.apagados.is_empty() {
            ui.label("Nada foi apagado: tudo o que as versões tiveram está no texto de agora.");
            return None;
        }
        if indices.is_empty() {
            ui.label("Nenhum trecho apagado tem essas palavras.");
            return None;
        }
        ui.label(
            RichText::new(if self.busca.trim().is_empty() {
                format!("{} apagados", trechos_por_extenso(indices.len()))
            } else {
                format!(
                    "{} com “{}”",
                    trechos_por_extenso(indices.len()),
                    self.busca.trim()
                )
            })
            .small()
            .weak(),
        );
        let altura = (ctx.content_rect().height() - 330.0).clamp(140.0, 440.0);
        let agora = chrono::Local::now();
        let mut acao = None;
        let mut copiar = None;
        ScrollArea::vertical()
            .id_salt("escritor-apagados")
            .max_height(altura)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (linha, &i) in indices.iter().take(LINHAS_MAXIMAS).enumerate() {
                    let a = &self.apagados[i];
                    ui.add(egui::Label::new(cortar(&a.trecho)).wrap());
                    let sumiu = match a.apagado_em {
                        Some(t) => format!("apagado {}", quando(agora, t)),
                        None => "apagado agora, ainda sem versão".to_owned(),
                    };
                    ui.label(
                        RichText::new(format!(
                            "{sumiu} · existia desde {} · em {}",
                            quando(agora, a.existiu_de),
                            if a.versoes == 1 {
                                "1 versão".to_owned()
                            } else {
                                format!("{} versões", a.versoes)
                            }
                        ))
                        .small()
                        .weak(),
                    );
                    ui.horizontal(|ui| {
                        let reinserir = ui.button("Reinserir no cursor");
                        ui_probe::record(
                            ctx,
                            format!("escritor-reinserir:{linha}"),
                            reinserir.rect,
                        );
                        if reinserir.clicked() {
                            acao = Some(Acao::Reinserir(a.trecho.clone()));
                        }
                        let copia = ui.button("Copiar");
                        ui_probe::record(ctx, format!("escritor-copiar:{linha}"), copia.rect);
                        if copia.clicked() {
                            copiar = Some(a.trecho.clone());
                        }
                    });
                    ui.separator();
                }
                if indices.len() > LINHAS_MAXIMAS {
                    ui.label(
                        RichText::new(format!(
                            "Mais {} trechos: use a busca para achar o que procura.",
                            indices.len() - LINHAS_MAXIMAS
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
        if let Some(texto) = copiar {
            ctx.copy_text(texto);
            self.aviso = Some("Copiado.".into());
        }
        acao
    }

    fn corpo_trecho(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> Option<Acao> {
        let Some((faixa, selecionado)) = self.selecao.clone() else {
            ui.label("Selecione um trecho da nota e abra de novo.");
            return None;
        };
        ui.label("Seleção:");
        ui.label(RichText::new(cortar(&selecionado)).italics());
        if !self.carregado {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Seguindo o trecho pelas versões…");
            });
            return None;
        }
        if self.trecho.is_empty() {
            ui.label("Este trecho não mudou em nenhuma versão guardada.");
            return None;
        }
        ui.label(
            RichText::new(if self.trecho.len() == 1 {
                "1 mudança, da mais nova para a mais antiga".to_owned()
            } else {
                format!(
                    "{} mudanças, da mais nova para a mais antiga",
                    self.trecho.len()
                )
            })
            .small()
            .weak(),
        );
        let altura = (ctx.content_rect().height() - 360.0).clamp(140.0, 420.0);
        let agora = chrono::Local::now();
        let mut acao = None;
        let mut copiar = None;
        ScrollArea::vertical()
            .id_salt("escritor-trecho")
            .max_height(altura)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (linha, m) in self.trecho.iter().enumerate() {
                    let quando_txt = match m.quando {
                        Some(t) => format!("Versão de {}", quando(agora, t)),
                        None => "Agora, ainda sem versão".to_owned(),
                    };
                    let nome = m
                        .nome
                        .as_ref()
                        .map_or(String::new(), |n| format!(" · “{n}”"));
                    let como = if m.nasceu { " · nasceu aqui" } else { "" };
                    ui.label(
                        RichText::new(format!("{quando_txt}{nome}{como}"))
                            .small()
                            .weak(),
                    );
                    ui.add(egui::Label::new(cortar(&m.texto)).wrap());
                    ui.horizontal(|ui| {
                        let copia = ui.button("Copiar");
                        ui_probe::record(
                            ctx,
                            format!("escritor-trecho-copiar:{linha}"),
                            copia.rect,
                        );
                        if copia.clicked() {
                            copiar = Some(m.texto.clone());
                        }
                        let igual = m.texto == selecionado;
                        let trocar =
                            ui.add_enabled(!igual, egui::Button::new("Trocar a seleção por esta"));
                        ui_probe::record(ctx, format!("escritor-trocar:{linha}"), trocar.rect);
                        if trocar.clicked() {
                            acao = Some(Acao::TrocarSelecao {
                                faixa: faixa.clone(),
                                esperado: selecionado.clone(),
                                texto: m.texto.clone(),
                            });
                        }
                    });
                    ui.separator();
                }
            });
        if let Some(texto) = copiar {
            ctx.copy_text(texto);
            self.aviso = Some("Copiado.".into());
        }
        acao
    }
}

/// Acessores só para os testes de GUI (`src/app/gui_tests/historico_h3h4.rs`).
#[cfg(test)]
impl Manager {
    pub(crate) fn carregou(&self) -> bool {
        self.carregado
    }
    pub(crate) fn vista(&self) -> Vista {
        self.vista
    }
    pub(crate) fn apagados(&self) -> &[Apagado] {
        &self.apagados
    }
    pub(crate) fn trecho(&self) -> &[MudancaDoTrecho] {
        &self.trecho
    }
}
