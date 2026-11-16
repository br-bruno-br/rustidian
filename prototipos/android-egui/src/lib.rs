#![cfg_attr(not(target_os = "android"), allow(dead_code))]
//! Protótipo R7a (egui): um editor multilinha, um botão "Salvar" e um rótulo de status.
//!
//! O objetivo é descobrir se o egui no Android digita bem pelo Gboard (acentos,
//! autocorretor, sugestões, apagar, selecionar, rolar, teclado sem cobrir o texto).
//! Roteiro de teste: docs/bruno/R7-TESTE.md. Achados: seção "R7a" do PLANO.md.

mod ime;

use eframe::egui;
use std::path::PathBuf;

struct Proto {
    id: egui::Id,
    texto: String,
    status: String,
    arquivo: PathBuf,
    /// Contadores para o dono relatar o que chegou: dá para ver se o IME está vivo.
    diag: Diag,
    #[cfg(target_os = "android")]
    android: android::Ponte,
}

#[derive(Default)]
struct Diag {
    eventos_ime: u32,
    ultimo: String,
}

impl Proto {
    fn new(cc: &eframe::CreationContext<'_>, dir: PathBuf, #[cfg(target_os = "android")] app: android_activity::AndroidApp) -> Self {
        // Letras maiores: o egui usa 12,5 pt por padrão, pequeno no celular.
        cc.egui_ctx.set_zoom_factor(1.3);
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
            diag: Diag::default(),
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

impl eframe::App for Proto {
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
                ctx.input_mut(|i| i.events.extend(novos.into_iter().map(traduz_para_egui)));
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
                "{} caracteres | eventos do teclado: {} | último: {}",
                self.texto.chars().count(),
                self.diag.eventos_ime,
                self.diag.ultimo
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
fn traduz_para_egui(e: ime::EventoIme) -> egui::Event {
    use egui::ImeEvent;
    egui::Event::Ime(match e {
        ime::EventoIme::LimparComposicao => ImeEvent::Preedit {
            text: String::new(),
            active_range_chars: None,
        },
        ime::EventoIme::ApagarAntes(n) => ImeEvent::DeleteSurrounding {
            before_chars: n,
            after_chars: 0,
        },
        ime::EventoIme::Confirmar(t) => ImeEvent::Commit(t),
        ime::EventoIme::Compor(t) => ImeEvent::Preedit {
            text: t,
            active_range_chars: None,
        },
    })
}

#[cfg(target_os = "android")]
mod android {
    use super::ime::{EstadoIme, EventoIme, traduzir};
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
                TextInputAction::Unspecified,
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
            let agora = EstadoIme {
                texto: s.text,
                composicao: s.compose_region.map(|r| (r.start, r.end)),
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
