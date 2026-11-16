#![cfg_attr(not(target_os = "android"), allow(dead_code))]
//! Protótipo R7a (egui): um editor multilinha, um botão "Salvar" e um rótulo de status.
//!
//! O objetivo é descobrir se o egui no Android digita bem pelo Gboard (acentos,
//! autocorretor, sugestões, apagar, selecionar, rolar, teclado sem cobrir o texto).
//! Roteiro de teste: docs/bruno/R7-TESTE.md. Achados: seção "R7a" do PLANO.md.

mod ime;
mod toque;

use eframe::egui;
use std::path::PathBuf;

struct Proto {
    id: egui::Id,
    texto: String,
    status: String,
    arquivo: PathBuf,
    /// Contadores para o dono relatar o que chegou: dá para ver se o IME está vivo.
    diag: Diag,
    filtro: toque::FiltroToque,
    #[cfg(target_os = "android")]
    android: android::Ponte,
}

#[derive(Default)]
struct Diag {
    eventos_ime: u32,
    ultimo: String,
    /// Últimas teclas "de verdade" (Enter, Backspace...) que o egui recebeu.
    teclas: String,
    emoji: String,
}

impl Proto {
    fn new(cc: &eframe::CreationContext<'_>, dir: PathBuf, #[cfg(target_os = "android")] app: android_activity::AndroidApp) -> Self {
        // Letras maiores: o egui usa 12,5 pt por padrão, pequeno no celular.
        cc.egui_ctx.set_zoom_factor(1.3);
        let emoji = carregar_emoji(&cc.egui_ctx);
        let arquivo = dir.join("nota.md");
        let (texto, status) = match std::fs::read_to_string(&arquivo) {
            Ok(t) => {
                let n = t.len();
                (t, format!("Lido de {} ({n} bytes)", arquivo.display()))
            }
            Err(_) => (String::new(), format!("Ainda sem arquivo em {}", arquivo.display())),
        };
        Self {
            id: egui::Id::unique("editor-nota"),
            texto,
            status,
            arquivo,
            diag: Diag {
                emoji,
                ..Diag::default()
            },
            filtro: toque::FiltroToque::default(),
            #[cfg(target_os = "android")]
            android: android::Ponte::nova(app),
        }
    }

    fn salvar(&mut self) {
        self.status = match std::fs::write(&self.arquivo, self.texto.as_bytes()) {
            Ok(()) => format!(
                "Salvo: {} bytes, {} caracteres em {}",
                self.texto.len(),
                self.texto.chars().count(),
                self.arquivo.display()
            ),
            Err(e) => format!("ERRO ao salvar: {e}"),
        };
    }
}

/// Carrega a fonte de emoji colorida do sistema como reserva (o egui só traz emoji de uma cor).
/// Devolve o que aconteceu, para aparecer na tela de diagnóstico.
fn carregar_emoji(ctx: &egui::Context) -> String {
    const CANDIDATAS: &[&str] = &[
        "/system/fonts/NotoColorEmoji.ttf",
        "/system/fonts/SamsungColorEmoji.ttf",
        "/system/fonts/ColorEmoji.ttf",
        "C:/Windows/Fonts/seguiemj.ttf", // só para testar no PC
    ];
    for caminho in CANDIDATAS {
        if let Ok(bytes) = std::fs::read(caminho) {
            let mut fontes = egui::FontDefinitions::default();
            fontes.font_data.insert(
                "emoji-sistema".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            for familia in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                // logo depois da fonte principal, antes do emoji de uma cor que vem com o egui
                if let Some(lista) = fontes.families.get_mut(&familia) {
                    lista.insert(1.min(lista.len()), "emoji-sistema".to_owned());
                }
            }
            ctx.set_fonts(fontes);
            return format!("emoji colorido: {caminho}");
        }
    }
    "emoji colorido: fonte do sistema não achada".to_owned()
}

impl eframe::App for Proto {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Toque: arrastar rola, toque longo seleciona (ver toque.rs).
        let agora = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
        let eventos = std::mem::take(&mut raw.events);
        raw.events = self.filtro.filtrar(eventos, agora);
        if self.filtro.aguardando() {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
        for e in &raw.events {
            if let egui::Event::Key {
                key, pressed: true, ..
            } = e
                && matches!(
                    key,
                    egui::Key::Enter | egui::Key::Backspace | egui::Key::Delete | egui::Key::Tab
                )
            {
                self.diag.teclas = format!("{key:?}");
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let id = self.id;
        let focado = ctx.memory(|m| m.has_focus(id));

        // Passo 1 (só Android): trazer o texto composto do Gboard para o egui.
        #[cfg(target_os = "android")]
        if focado {
            let novos = self.android.consultar();
            if !novos.is_empty() {
                self.diag.eventos_ime += novos.len() as u32;
                self.diag.ultimo = format!("{novos:?}");
                ctx.input_mut(|i| i.events.extend(novos.into_iter().flat_map(traduz_para_egui)));
            }
            // O winit não nos acorda quando o teclado muda: consultamos a cada ~30 ms
            // enquanto o editor tem foco.
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        #[cfg(not(target_os = "android"))]
        let _ = focado;

        egui::Panel::top("topo").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Proto egui");
                if ui.button("Salvar").clicked() {
                    self.salvar();
                }
            });
            ui.label(&self.status);
            ui.add_space(4.0);
        });

        egui::Panel::bottom("diag").show(ui, |ui| {
            ui.small(format!(
                "{} caracteres | eventos do teclado: {} | último: {} | tecla: {} | {}",
                self.texto.chars().count(),
                self.diag.eventos_ime,
                self.diag.ultimo,
                self.diag.teclas,
                self.diag.emoji
            ));
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut self.texto)
                            .id(id)
                            .hint_text("Toque aqui e digite")
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .lock_focus(true),
                    );
                    // Toque no editor: reabre o teclado (se foi fechado pelo botão "voltar") e
                    // zera a memória do Gboard, porque o cursor do egui pode ter mudado de lugar.
                    #[cfg(target_os = "android")]
                    if resp.clicked() || resp.gained_focus() {
                        self.android.mostrar_teclado();
                    }
                    #[cfg(not(target_os = "android"))]
                    let _ = resp;
                });
        });
    }
}

#[cfg(target_os = "android")]
fn traduz_para_egui(e: ime::EventoIme) -> Vec<egui::Event> {
    use egui::ImeEvent;
    let ime = |e: ImeEvent| vec![egui::Event::Ime(e)];
    match e {
        ime::EventoIme::LimparComposicao => ime(ImeEvent::Preedit {
            text: String::new(),
            active_range_chars: None,
        }),
        ime::EventoIme::ApagarAntes(n) => ime(ImeEvent::DeleteSurrounding {
            before_chars: n,
            after_chars: 0,
        }),
        ime::EventoIme::Confirmar(t) => ime(ImeEvent::Commit(t)),
        ime::EventoIme::Compor(t) => ime(ImeEvent::Preedit {
            text: t,
            active_range_chars: None,
        }),
        // O TextEdit ignora o Commit de uma quebra de linha; ela só entra pela tecla Enter.
        ime::EventoIme::Enter => [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect(),
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::ime::{EstadoIme, EventoIme, mutf8_para_utf8, traduzir};
    use android_activity::{
        AndroidApp,
        input::{ImeOptions, InputType, TextInputAction, TextInputState, TextSpan},
    };

    /// Liga o egui ao teclado do Android (GameActivity/GameTextInput).
    pub struct Ponte {
        app: AndroidApp,
        anterior: EstadoIme,
    }

    impl Ponte {
        pub fn nova(app: AndroidApp) -> Self {
            // Diz ao Gboard que é texto livre, multilinha e com autocorretor. Sem isso o
            // teclado pode abrir sem sugestões.
            app.set_ime_editor_info(
                InputType::TYPE_CLASS_TEXT
                    | InputType::TYPE_TEXT_FLAG_MULTI_LINE
                    | InputType::TYPE_TEXT_FLAG_AUTO_CORRECT
                    | InputType::TYPE_TEXT_FLAG_CAP_SENTENCES,
                // None = a tecla de ação do teclado vira Enter (nova linha), em vez de "ir/enviar".
                TextInputAction::None,
                ImeOptions::IME_FLAG_NO_FULLSCREEN | ImeOptions::IMG_FLAG_NO_EXTRACT_UI,
            );
            Self {
                app,
                anterior: EstadoIme::default(),
            }
        }

        /// Lê o estado atual do Gboard e devolve o que mudou desde a última leitura.
        pub fn consultar(&mut self) -> Vec<EventoIme> {
            let s = self.app.text_input_state();
            let composicao = s.compose_region.map(|r| {
                (
                    mutf8_para_utf8(&s.text, r.start),
                    mutf8_para_utf8(&s.text, r.end),
                )
            });
            let agora = EstadoIme {
                texto: s.text,
                composicao,
            };
            let eventos = traduzir(&self.anterior, &agora);
            self.anterior = agora;
            eventos
        }

        /// Mostra o teclado e esvazia a memória dele (o cursor do egui pode ter ido para
        /// outro lugar; o teclado só precisa saber da palavra que está sendo digitada).
        pub fn mostrar_teclado(&mut self) {
            if self.anterior.composicao.is_none() {
                self.app.set_text_input_state(TextInputState {
                    text: String::new(),
                    selection: TextSpan { start: 0, end: 0 },
                    compose_region: None,
                });
                self.anterior = EstadoIme::default();
            }
            self.app.show_soft_input(true);
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("rustidian"),
    );
    let dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp"));
    let opcoes = eframe::NativeOptions {
        android_app: Some(app.clone()),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Rustidian proto egui",
        opcoes,
        Box::new(move |cc| Ok(Box::new(Proto::new(cc, dir, app)))),
    ) {
        log::error!("eframe falhou: {e}");
    }
}
