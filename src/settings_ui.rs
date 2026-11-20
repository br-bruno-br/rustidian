use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct EditorPreferences {
    pub reading_view: bool,
    pub source_mode: bool,
    pub show_mode: bool,
    pub inline_title: bool,
    pub readable_line_length: bool,
    /// Download images that notes name by web address. Off by default: opening a note must not
    /// call a server the note names.
    pub remote_images: bool,
    /// Sublinhar palavras desconhecidas (R10). Vale só quando o dicionario esta instalado.
    pub spellcheck: bool,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            reading_view: false,
            source_mode: false,
            show_mode: true,
            inline_title: true,
            readable_line_length: true,
            remote_images: false,
            spellcheck: true,
        }
    }
}

impl EditorPreferences {
    pub fn apply(&self, tab: &mut Tab) {
        if !tab.document.is_image() && !tab.document.is_plain_text() {
            tab.set_view(if self.source_mode {
                View::Source
            } else {
                View::Live
            });
            if self.reading_view {
                tab.set_view(View::Read);
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Appearance,
    Interface,
    Editor,
    Files,
    Hotkeys,
    Sync,
    Daily,
    Templates,
    Unique,
    Recovery,
}

impl Page {
    const ALL: [Self; 10] = [
        Self::Appearance,
        Self::Interface,
        Self::Editor,
        Self::Files,
        Self::Hotkeys,
        Self::Sync,
        Self::Daily,
        Self::Templates,
        Self::Unique,
        Self::Recovery,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Interface => "Interface",
            Self::Editor => "Editor",
            Self::Files => "Files and links",
            Self::Hotkeys => "Hotkeys",
            Self::Sync => "Sync",
            Self::Daily => "Daily notes",
            Self::Templates => "Templates",
            Self::Unique => "Unique note creator",
            Self::Recovery => "File recovery",
        }
    }

    /// The page name shown; `label` stays English because the screen test finds pages by it.
    fn shown(self) -> &'static str {
        match self {
            Self::Appearance => "Aparência",
            Self::Interface => "Interface",
            Self::Editor => "Editor",
            Self::Files => "Arquivos e links",
            Self::Hotkeys => "Atalhos",
            Self::Sync => "Sincronização",
            Self::Daily => "Notas diárias",
            Self::Templates => "Modelos",
            Self::Unique => "Criador de nota única",
            Self::Recovery => "Recuperação de arquivos",
        }
    }

    fn matches(self, query: &str) -> bool {
        let terms = match self {
            Self::Appearance => {
                "appearance base color scheme theme system dark light font text size interface monospace family aparência aparencia cor esquema tema escuro claro fonte tamanho texto"
            }
            Self::Interface => "interface zoom nível",
            Self::Editor => {
                "editor spelling spell check corretor ortografia dicionario editing files save autosave default view source live preview inline title readable line length status mode privacy images internet web remote load edição edicao modo padrão título linha largura privacidade imagens salvar salvamento automático"
            }
            Self::Files => {
                "files and links automatically update move folder arquivos links atualizar automaticamente mover pasta lixeira"
            }
            Self::Hotkeys => "hotkeys keyboard shortcuts commands atalhos teclado comandos",
            Self::Sync => {
                "sync synchronization server address token passphrase device connect disconnect keys devices sincronização sincronizar servidor endereço senha dispositivo conectar"
            }
            Self::Daily => {
                "daily notes date format folder template new file location notas diárias data formato pasta modelo local"
            }
            Self::Templates => {
                "templates date time format folder location modelos data hora formato pasta local"
            }
            Self::Unique => {
                "unique notes prefix format folder template new file location notas únicas prefixo formato pasta modelo local"
            }
            Self::Recovery => {
                "file recovery snapshots history interval length days keep recuperação recuperacao arquivos versões histórico intervalo dias"
            }
        };
        query.split_whitespace().all(|word| terms.contains(word))
    }
}

impl NotesApp {
    fn editor_settings(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        group(ui, |ui| {
            changed |= choice(
                ui,
                "Modo padrão para novas abas",
                "O modo em que uma nova aba Markdown é aberta.",
                "settings-default-view",
                &mut self.editor_preferences.reading_view,
                ["Editing view", "Reading view"],
            );
            ui.separator();
            changed |= choice(
                ui,
                "Modo de edição padrão",
                "O modo de edição com que uma nova aba começa.",
                "settings-editing-mode",
                &mut self.editor_preferences.source_mode,
                ["Live Preview", "Source mode"],
            );
            ui.separator();
            changed |= toggle(
                ui,
                "Mostrar o modo de edição na barra de status",
                "Mostra o modo de edição na barra de status.",
                "settings-show-mode",
                &mut self.editor_preferences.show_mode,
            );
        });
        ui.add_space(crate::theme::space::XL);
        ui.strong("Exibição");
        ui.add_space(crate::theme::space::S);
        group(ui, |ui| {
            changed |= toggle(
                ui,
                "Título embutido",
                "Mostra o nome do arquivo como um título editável junto ao conteúdo.",
                "settings-inline-title",
                &mut self.editor_preferences.inline_title,
            );
            ui.separator();
            changed |= toggle(
                ui,
                "Largura de linha confortável",
                "Limita a largura máxima das linhas. Cabe menos conteúdo na tela, mas blocos longos de texto ficam mais legíveis.",
                "settings-readable-line-length",
                &mut self.editor_preferences.readable_line_length,
            );
        });
        ui.add_space(crate::theme::space::XL);
        ui.strong("Privacidade");
        ui.add_space(crate::theme::space::S);
        group(ui, |ui| {
            changed |= toggle(
                ui,
                "Carregar imagens da internet",
                "Desligado: notas com imagens da web (http ou https) não fazem nenhum pedido de rede; um marcador toma o lugar e um clique carrega aquela imagem.",
                "settings-remote-images",
                &mut self.editor_preferences.remote_images,
            );
        });
        ui.add_space(crate::theme::space::XL);
        ui.strong("Corretor ortográfico");
        ui.add_space(crate::theme::space::S);
        changed |= self.spelling_settings(ui);
        ui.add_space(crate::theme::space::XL);
        ui.strong("Salvamento");
        ui.add_space(crate::theme::space::S);
        group(ui, |ui| {
            changed |= toggle(
                ui,
                "Salvar automaticamente",
                "Salva as alterações após 600 ms sem digitar.",
                "settings-autosave",
                &mut self.autosave_enabled,
            );
        });
        changed
    }

    /// O corretor ortografico: liga/desliga e o dicionario pt-BR (baixado uma vez, fora do binario).
    fn spelling_settings(&mut self, ui: &mut egui::Ui) -> bool {
        use rustidian::spell::Status;
        let mut changed = false;
        group(ui, |ui| {
            changed |= toggle(
                ui,
                "Corrigir ortografia (pt-BR)",
                "Sublinha as palavras que o dicionário não conhece. Só em prosa: front matter, código, matemática, links e números ficam de fora.",
                "settings-spellcheck",
                &mut self.editor_preferences.spellcheck,
            );
            ui.separator();
            let spelling = &self.editor.spelling;
            let installed = spelling.is_installed();
            let status = spelling.status().clone();
            let description = match (&status, &spelling.message) {
                (_, Some(error)) => format!("Não deu certo: {error}"),
                (Status::Downloading, _) => "Baixando o dicionário…".to_string(),
                (Status::Loading, _) => "Carregando o dicionário…".to_string(),
                (Status::Ready, _) => {
                    "Instalado (VERO, do LibreOffice). Fica na pasta de dados do app, fora do programa.".to_string()
                }
                _ if installed => "Instalado. Ligue o corretor para usar.".to_string(),
                _ => "Não instalado. São uns 5 MB, baixados uma vez do repositório do LibreOffice e conferidos por sha256.".to_string(),
            };
            let can_download =
                !installed && !matches!(status, Status::Downloading | Status::Loading);
            let retry = installed && self.editor.spelling.message.is_some();
            row(ui, "Dicionário pt-BR", &description, |ui| {
                if can_download || retry {
                    let label = if retry {
                        "Tentar de novo"
                    } else {
                        "Baixar dicionário pt-BR"
                    };
                    let response = ui.button(label);
                    crate::ui_probe::record(
                        ui.ctx(),
                        "settings-spellcheck-download",
                        response.rect,
                    );
                    if response.clicked() {
                        self.editor.spelling.start_download();
                    }
                }
                false
            });
        });
        changed
    }

    pub(super) fn settings_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        close: &mut bool,
        recovery_action: &mut Option<bool>,
    ) {
        theme::dialog_style(ui);
        ui.style_mut().spacing.slider_width = 120.0;
        ui.style_mut().visuals.faint_bg_color = if self.dark {
            Color32::from_rgb(35, 35, 35)
        } else {
            Color32::from_rgb(249, 249, 249)
        };
        let width = (ctx.content_rect().width() - 48.0).clamp(360.0, 900.0);
        let height = (ctx.content_rect().height() - 100.0).clamp(260.0, 672.0);
        ui.set_width(width);
        let before = (
            self.dark,
            self.appearance,
            self.palette,
            self.ui_zoom,
            self.document_font_size,
            self.fonts.clone(),
            self.editor_preferences.clone(),
            self.autosave_enabled,
            self.note_aids.clone(),
            self.recovery_config.clone(),
            self.always_update_links,
            self.confirm_trash,
        );
        let mut changed = false;
        ui.horizontal_top(|ui| {
            egui::Frame::new()
                .fill(if self.dark {
                    Color32::from_rgb(38, 38, 38)
                } else {
                    Color32::from_rgb(246, 246, 246)
                })
                .inner_margin(crate::theme::space::margin(crate::theme::space::M, crate::theme::space::M))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(if width < 600.0 { 152.0 } else { 188.0 });
                        ui.spacing_mut().item_spacing.y = crate::theme::space::XS;
                        ui.set_min_height(height - 24.0);
                        let input = ui.add(
                            TextEdit::singleline(&mut self.dialog_input)
                                .hint_text("Buscar configurações…")
                                .desired_width(f32::INFINITY),
                        );
                        crate::ui_probe::record(ctx, "dialog-input", input.rect);
                        if self.dialog_focus {
                            input.request_focus();
                            self.dialog_focus = false;
                        }
                        if input.has_focus() {
                            crate::ui_probe::record(ctx, "dialog-input-focused", input.rect);
                        }
                        let query = self.dialog_input.to_lowercase();
                        if !query.is_empty()
                            && !self.settings_page.matches(&query)
                            && let Some(page) =
                                Page::ALL.into_iter().find(|page| page.matches(&query))
                        {
                            self.settings_page = page;
                        }
                        ui.add_space(crate::theme::space::M);
                        ScrollArea::vertical()
                            .id_salt("settings-navigation")
                            .max_height(height - 60.0)
                            .show(ui, |ui| {
                                for page in Page::ALL {
                                    if page == Page::Appearance {
                                        ui.weak("Opções");
                                        ui.add_space(crate::theme::space::XS);
                                    }
                                    if page == Page::Daily {
                                        ui.add_space(crate::theme::space::M);
                                        ui.weak("Plugins essenciais");
                                        ui.add_space(crate::theme::space::XS);
                                    }
                                    if page.matches(&query) {
                                        let response =
                                            navigation_row(ui, page, page == self.settings_page);
                                        crate::ui_probe::record(
                                            ctx,
                                            format!("settings-page:{}", page.label()),
                                            response.rect,
                                        );
                                        if response.clicked() {
                                            self.settings_page = page;
                                        }
                                    }
                                }
                            });
                    });
                });
            ui.add_space(crate::theme::space::L);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 24.0).max(140.0));
                ui.set_min_height(height);
                ui.add_space(crate::theme::space::XL);
                let button = theme::dialog_header(ui, "Configurações", "Fechar configurações");
                crate::ui_probe::record(ctx, "settings-close", button.rect);
                *close |= button.clicked();
                ui.add_space(crate::theme::space::M);
                let query = self.dialog_input.to_lowercase();
                if !self.settings_page.matches(&query) {
                    ui.weak("Nenhuma configuração encontrada.");
                    return;
                }
                ctx.data_mut(|data| data.insert_temp(egui::Id::unique("settings-query"), query.clone()));
                let content = ScrollArea::vertical()
                    .id_salt(("settings-content", self.settings_page as u8))
                    .max_height(height - 100.0)
                    .show(ui, |ui| {
                        if self.settings_page == Page::Editor {
                            changed |= self.editor_settings(ui);
                            return;
                        }
                        if self.settings_page == Page::Sync {
                            self.sync_settings(ui, ctx);
                            return;
                        }
                        if self.settings_page == Page::Appearance {
                            changed |= theme::palette::picker(ui, &mut self.palette, self.dark);
                        }
                        group(ui, |ui| match self.settings_page {
                            Page::Appearance => {
                                changed |=
                                    row(
                                        ui,
                                        "Esquema de cores",
                                        "Siga o sistema ou escolha uma aparência clara ou escura.",
                                        |ui| {
                                            let before = self.appearance;
                                            let response = egui::ComboBox::from_id_salt(
                                                "settings-color-scheme",
                                            )
                                            .icon(theme::combo_chevron)
                                            .selected_text(rustidian::i18n::t(self.appearance.label()))
                                            .show_ui(ui, |ui| {
                                                theme::menu_style(ui);
                                                for appearance in [theme::Appearance::System, theme::Appearance::Dark, theme::Appearance::Light] {
                                                    let label = appearance.label();
                                                    let response = ui.selectable_value(
                                                        &mut self.appearance,
                                                        appearance,
                                                        rustidian::i18n::t(label),
                                                    );
                                                    crate::ui_probe::record(
                                                        ctx,
                                                        format!("settings-theme:{}", label),
                                                        response.rect,
                                                    );
                                                }
                                            });
                                            crate::ui_probe::record(
                                                ctx,
                                                "settings-dark",
                                                response.response.rect,
                                            );
                                            self.appearance != before
                                        },
                                    );
                                ui.separator();
                                changed |= row(
                                    ui,
                                    "Tamanho da fonte",
                                    "Tamanho da fonte em pixels, que vale para a edição e a leitura.",
                                    |ui| {
                                        let response = theme::slider(
                                            ui,
                                            egui::Slider::new(&mut self.document_font_size, 10..=30),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-font-size",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                                for (title, description, id, value, mono) in [
                                    (
                                        "Fonte da interface",
                                        "Fonte de menus, painéis laterais e diálogos.",
                                        "settings-interface-font",
                                        &mut self.fonts.interface,
                                        false,
                                    ),
                                    (
                                        "Fonte do texto",
                                        "Fonte da edição e da leitura.",
                                        "settings-text-font",
                                        &mut self.fonts.text,
                                        false,
                                    ),
                                    (
                                        "Fonte monoespaçada",
                                        "Fonte de blocos de código e de código em linha.",
                                        "settings-monospace-font",
                                        &mut self.fonts.monospace,
                                        true,
                                    ),
                                ] {
                                    ui.separator();
                                    let missing = self.missing_fonts.iter().any(|name| name == &*value);
                                    let description = if missing {
                                        format!("{value} não está instalada; a fonte padrão será usada.")
                                    } else {
                                        description.to_owned()
                                    };
                                    changed |= row(ui, title, &description, |ui| {
                                        font_choice(ui, id, value, mono)
                                    });
                                }
                            }
                            Page::Interface => {
                                changed |= row(
                                    ui,
                                    "Nível de zoom",
                                    "Redimensiona a interface e o documento juntos.",
                                    |ui| {
                                        let response = theme::slider(
                                            ui,
                                            egui::Slider::new(&mut self.ui_zoom, 0.75..=1.5)
                                                .step_by(0.05),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-zoom",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                            }
                            Page::Editor => unreachable!(),
                            Page::Sync => unreachable!(),
                            Page::Files => {
                                changed |= toggle(
                                    ui,
                                    "Atualizar links internos automaticamente",
                                    "Atualiza os links ao mover ou renomear arquivos.",
                                    "settings-update-move-links",
                                    &mut self.always_update_links,
                                );
                                ui.separator();
                                changed |= toggle(
                                    ui,
                                    "Confirmar antes de mover para a Lixeira",
                                    "Mostra uma confirmação antes de mover arquivos ou pastas para a Lixeira.",
                                    "settings-confirm-trash",
                                    &mut self.confirm_trash,
                                );
                            }
                            Page::Hotkeys => {
                                for command in rustidian::commands::ranked("", Scope::Editor) {
                                    if !command.shortcut.is_empty() {
                                        row(ui, rustidian::commands::label(command.name), "", |ui| {
                                            ui.weak(command.shortcut);
                                            false
                                        });
                                        ui.separator();
                                    }
                                }
                            }
                            Page::Daily => {
                                changed |= text(
                                    ui,
                                    "Formato da data",
                                    "Como os nomes dos arquivos das notas diárias são formatados.",
                                    "settings-daily-format",
                                    &mut self.note_aids.daily.format,
                                    "YYYY-MM-DD",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Local dos novos arquivos",
                                    "Pasta das novas notas diárias.",
                                    "settings-daily-folder",
                                    &mut self.note_aids.daily.folder,
                                    "Pasta do cofre",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Local do arquivo de modelo",
                                    "Uma nota para usar como modelo diário.",
                                    "settings-daily-template",
                                    &mut self.note_aids.daily.template,
                                    "Nota de modelo (opcional)",
                                );
                            }
                            Page::Templates => {
                                changed |= text(
                                    ui,
                                    "Local da pasta de modelos",
                                    "Pasta que contém seus modelos.",
                                    "settings-template-folder",
                                    &mut self.note_aids.templates.folder,
                                    "Escolha uma pasta do cofre",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Formato da data",
                                    "Formato da variável de data.",
                                    "settings-template-date",
                                    &mut self.note_aids.templates.date_format,
                                    "YYYY-MM-DD",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Formato da hora",
                                    "Formato da variável de hora.",
                                    "settings-template-time",
                                    &mut self.note_aids.templates.time_format,
                                    "HH:mm",
                                );
                            }
                            Page::Unique => {
                                changed |= text(
                                    ui,
                                    "Local dos novos arquivos",
                                    "Pasta das notas únicas.",
                                    "settings-unique-folder",
                                    &mut self.note_aids.unique.folder,
                                    "Pasta do cofre",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Local do arquivo de modelo",
                                    "Uma nota para usar como modelo.",
                                    "settings-unique-template",
                                    &mut self.note_aids.unique.template,
                                    "Nota de modelo (opcional)",
                                );
                                ui.separator();
                                changed |= text(
                                    ui,
                                    "Formato do prefixo único",
                                    "Formato da data nos nomes das notas únicas.",
                                    "settings-unique-format",
                                    &mut self.note_aids.unique.format,
                                    "YYYYMMDDHHmm",
                                );
                            }
                            Page::Recovery => {
                                changed |= toggle(
                                    ui,
                                    "Manter o histórico de recuperação de arquivos",
                                    "Desligado: nada é gravado, e o histórico já salvo é apagado. O histórico fica em .rustidian/ no cofre e não é sincronizado.",
                                    "settings-recovery-enabled",
                                    &mut self.recovery_config.enabled,
                                );
                                ui.separator();
                                changed |= row(
                                    ui,
                                    "Intervalo das versões antigas",
                                    "Passado o período abaixo, fica 1 versão a cada N minutos até 7 dias; depois 1 por dia até 30 dias e 1 por semana para sempre. Com 0, todas ficam até 7 dias.",
                                    |ui| {
                                        let response = ui.add(
                                            egui::DragValue::new(
                                                &mut self.recovery_config.interval_minutes,
                                            )
                                            .range(0.0..=f64::MAX)
                                            .speed(0.1)
                                            .suffix(" min"),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-interval",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                                ui.separator();
                                changed |= row(
                                    ui,
                                    "Guardar todas as versões por",
                                    "Nesse período nenhuma versão sai. Versões com nome nunca saem. A mudança vale depois de 1 minuto.",
                                    |ui| {
                                        let response = ui.add(
                                            egui::DragValue::new(
                                                &mut self.recovery_config.keep_days,
                                            )
                                            .range(1.0..=f64::MAX)
                                            .suffix(" dias"),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-days",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                                ui.separator();
                                changed |= row(
                                    ui,
                                    "Passos do desfazer por nota",
                                    "O Ctrl+Z que fica depois de fechar a nota: quantos passos guardar. Passando, os mais antigos saem. Ver os galhos: Ctrl+P, \"Galhos do desfazer\".",
                                    |ui| {
                                        let response = ui.add(
                                            egui::DragValue::new(
                                                &mut self.recovery_config.undo_max_steps,
                                            )
                                            .range(100..=1_000_000)
                                            .speed(50.0)
                                            .suffix(" passos"),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-undo-steps",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                                ui.separator();
                                changed |= row(
                                    ui,
                                    "Espaço do desfazer por nota",
                                    "Teto de espaço do desfazer guardado de cada nota (o que bater primeiro, passos ou espaço).",
                                    |ui| {
                                        let response = ui.add(
                                            egui::DragValue::new(
                                                &mut self.recovery_config.undo_max_kb,
                                            )
                                            .range(64..=524_288)
                                            .speed(16.0)
                                            .suffix(" KB"),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-undo-kb",
                                            response.rect,
                                        );
                                        response.changed()
                                    },
                                );
                                ui.separator();
                                {
                                    let ligado = self.recovery_config.enabled;
                                    self.espaco.quadro(
                                        ctx,
                                        self.historico.as_ref(),
                                        ligado,
                                        self.recovery_manager.active(),
                                    );
                                    let rotulo = self.espaco.rotulo(ligado);
                                    let ocupado = self.espaco.compactando();
                                    let pronto = ligado
                                        && !ocupado
                                        && !self.espaco.calculando()
                                        && self.historico.is_some();
                                    let aviso = self.espaco.aviso.clone();
                                    let mut compactar = false;
                                    row(ui, "Espaço", &rotulo, |ui| {
                                        let botao = ui.add_enabled(
                                            pronto,
                                            egui::Button::new("Compactar agora"),
                                        );
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-compact",
                                            botao.rect,
                                        );
                                        compactar = botao.clicked();
                                        if ocupado {
                                            ui.spinner();
                                        }
                                        false
                                    });
                                    if let Some(aviso) = aviso
                                        && ligado
                                    {
                                        ui.label(RichText::new(aviso).size(13.0).weak());
                                    }
                                    if compactar && let Some(historico) = &self.historico {
                                        self.espaco.compactar(historico);
                                    }
                                }
                                ui.separator();
                                row(
                                    ui,
                                    "Versões",
                                    "Veja ou apague o histórico de recuperação salvo.",
                                    |ui| {
                                        let ready = !self.recovery_manager.active();
                                        let clear =
                                            ui.add_enabled(ready, egui::Button::new("Limpar"));
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-clear",
                                            clear.rect,
                                        );
                                        if clear.clicked() {
                                            *recovery_action = Some(true);
                                        }
                                        let view = ui.add_enabled(ready, egui::Button::new("Ver"));
                                        crate::ui_probe::record(
                                            ctx,
                                            "settings-recovery-view",
                                            view.rect,
                                        );
                                        if ready {
                                            crate::ui_probe::record(
                                                ctx,
                                                "settings-recovery-view-ready",
                                                view.rect,
                                            );
                                        }
                                        if view.clicked() {
                                            *recovery_action = Some(false);
                                        }
                                        false
                                    },
                                );
                            }
                        });
                        if self.settings_page == Page::Hotkeys {
                            ui.add_space(crate::theme::space::M);
                            ui.weak("A personalização de atalhos ainda não está disponível.");
                        }
                    });
                crate::ui_probe::record(ctx, "settings-viewport", content.inner_rect);
                if let Some(error) = &self.error {
                    ui.add_space(crate::theme::space::M);
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    crate::ui_probe::record(ctx, "dialog-error", ui.min_rect());
                }
            });
        });
        if changed {
            if self.fonts != before.5 {
                self.missing_fonts = theme::set_fonts(ctx, &self.fonts);
            }
            theme::palette::select(ctx, self.palette);
            theme::apply_appearance(ctx, self.appearance);
            self.dark = ctx.theme() == egui::Theme::Dark;
            if let Err(error) = self.write_session() {
                (
                    self.dark,
                    self.appearance,
                    self.palette,
                    self.ui_zoom,
                    self.document_font_size,
                    self.fonts,
                    self.editor_preferences,
                    self.autosave_enabled,
                    self.note_aids,
                    self.recovery_config,
                    self.always_update_links,
                    self.confirm_trash,
                ) = before;
                self.error = Some(format!("As configurações não foram salvas: {error}"));
                self.missing_fonts = theme::set_fonts(ctx, &self.fonts);
                theme::palette::select(ctx, self.palette);
                theme::apply_appearance(ctx, self.appearance);
            } else {
                if self.palette != before.2 {
                    theme::palette::remember(self.palette);
                }
                if self.appearance != before.1 {
                    theme::palette::remember_appearance(self.appearance);
                }
                self.settings_saved();
            }
            ctx.set_zoom_factor(self.ui_zoom);
            ctx.request_repaint();
        }
    }
}

pub(super) fn row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    ui.add_space(crate::theme::space::M);
    let label_width = (ui.available_width() - 180.0).max(100.0);
    let changed = ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, 0.0),
            Layout::top_down(Align::LEFT),
            |ui| {
                ui.spacing_mut().item_spacing.y = crate::theme::space::XS;
                ui.label(title);
                if !description.is_empty() {
                    ui.label(RichText::new(description).size(13.0).weak());
                }
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), control)
            .inner
    });
    reveal_match(ui, title, changed.response.rect);
    ui.add_space(crate::theme::space::M);
    changed.inner
}

/// Searching settings scrolls the first matching row into view, once per query.
fn reveal_match(ui: &egui::Ui, title: &str, rect: egui::Rect) {
    let (query, shown): (String, Option<String>) = ui.ctx().data(|data| {
        (
            data.get_temp(egui::Id::unique("settings-query"))
                .unwrap_or_default(),
            data.get_temp(egui::Id::unique("settings-query-shown")),
        )
    });
    let title = title.to_lowercase();
    if query.is_empty()
        || shown.as_deref() == Some(query.as_str())
        || !query.split_whitespace().all(|word| title.contains(word))
    {
        return;
    }
    ui.scroll_to_rect(rect, Some(Align::Center));
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::unique("settings-query-shown"), query));
}

/// An installed font family, or the default (empty), with a search field.
fn font_choice(ui: &mut egui::Ui, id: &str, value: &mut String, mono: bool) -> bool {
    let before = value.clone();
    let search_id = egui::Id::unique(id).with("search");
    let response = egui::ComboBox::from_id_salt(id)
        .icon(theme::combo_chevron)
        .width(180.0)
        .height(320.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .selected_text(if value.is_empty() {
            rustidian::i18n::t("Default")
        } else {
            value.as_str()
        })
        .show_ui(ui, |ui| {
            theme::menu_style(ui);
            // Each opening starts with an empty, focused search.
            let opened = ui
                .data_mut(|data| data.remove_temp::<bool>(search_id.with("opened")))
                .is_some();
            let mut search: String = if opened {
                String::new()
            } else {
                ui.data(|data| data.get_temp(search_id)).unwrap_or_default()
            };
            let field = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Buscar fontes")
                    .desired_width(f32::INFINITY),
            );
            if opened {
                field.request_focus();
            }
            crate::ui_probe::record(ui.ctx(), format!("{id}-search"), field.rect);
            ui.data_mut(|data| data.insert_temp(search_id, search.clone()));
            let search = search.to_lowercase();
            let names = std::iter::once("").chain(
                theme::fonts::installed(mono)
                    .iter()
                    .map(String::as_str)
                    .filter(|name| name.to_lowercase().contains(&search)),
            );
            for name in names {
                let label = if name.is_empty() { "Default" } else { name };
                let response = ui.selectable_label(*value == name, rustidian::i18n::t(label));
                crate::ui_probe::record(ui.ctx(), format!("{id}:{label}"), response.rect);
                if response.clicked() {
                    *value = name.to_owned();
                    ui.close();
                }
            }
        });
    if response.response.clicked() {
        ui.data_mut(|data| data.insert_temp(search_id.with("opened"), true));
    }
    crate::ui_probe::record(ui.ctx(), id, response.response.rect);
    *value != before
}

pub(super) fn toggle(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    id: &str,
    value: &mut bool,
) -> bool {
    row(ui, title, description, |ui| {
        let response = switch(ui, value);
        crate::ui_probe::record(ui.ctx(), id, response.rect);
        response.changed()
    })
}

fn text(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    id: &str,
    value: &mut String,
    hint: &str,
) -> bool {
    row(ui, title, description, |ui| {
        let response = ui.add(
            TextEdit::singleline(value)
                .id_salt(id)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        crate::ui_probe::record(ui.ctx(), id, response.rect);
        response.changed()
    })
}

fn navigation_row(ui: &mut egui::Ui, page: Page, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 25.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, page.shown())
    });
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            rect,
            6,
            if selected {
                Color32::from_gray(if ui.visuals().dark_mode { 45 } else { 216 })
            } else {
                theme::hover_fill(ui)
            },
        );
    }
    let icon = match page {
        Page::Appearance => Icon::Appearance,
        Page::Interface => Icon::SidebarLeft,
        Page::Editor => Icon::Edit,
        Page::Files => Icon::Files,
        Page::Hotkeys => Icon::Commands,
        Page::Sync => Icon::OpenLink,
        Page::Daily => Icon::DailyNote,
        Page::Templates => Icon::Templates,
        Page::Unique => Icon::NewNote,
        Page::Recovery => Icon::Recovery,
    };
    theme::icon_image(icon, ui.visuals().weak_text_color()).paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        ),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        page.shown(),
        FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
    response
}

fn switch(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(40.0, 22.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *value, "")
    });
    let fill = if *value {
        theme::accent(ui)
    } else {
        Color32::from_gray(if ui.visuals().dark_mode { 63 } else { 190 })
    };
    ui.painter().rect_filled(rect, 11, fill);
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                rect.right() - 11.0
            } else {
                rect.left() + 11.0
            },
            rect.center().y,
        ),
        9.0,
        Color32::WHITE,
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            13,
            Stroke::new(1.0, theme::accent(ui)),
            egui::StrokeKind::Outside,
        );
    }
    response
}

fn choice(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    id: &str,
    value: &mut bool,
    labels: [&str; 2],
) -> bool {
    row(ui, title, description, |ui| {
        let before = *value;
        let response = egui::ComboBox::from_id_salt(id)
            .icon(theme::combo_chevron)
            .selected_text(rustidian::i18n::t(labels[usize::from(*value)]))
            .show_ui(ui, |ui| {
                theme::menu_style(ui);
                for (i, label) in labels.into_iter().enumerate() {
                    let r = ui.selectable_value(value, i == 1, rustidian::i18n::t(label));
                    crate::ui_probe::record(ui.ctx(), format!("{id}:{label}"), r.rect);
                }
            });
        crate::ui_probe::record(ui.ctx(), id, response.response.rect);
        *value != before
    })
}

pub(super) fn group<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(12)
        .inner_margin(crate::theme::space::margin(crate::theme::space::XL, 0.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(ui)
        })
        .inner
}
