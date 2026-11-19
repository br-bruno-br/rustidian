//! Liga o editor ao histórico das notas (H1). O quadro só compara texto com o que já mandou e,
//! quando mudou, entrega uma cópia à linha de fundo (`rustidian::historico_fundo`); o agrupador
//! "como o Word", o blake3, o zstd e o `redb` ficam lá, nunca no quadro.
use super::*;
use rustidian::historico_fundo::{Historico, Origem, Ritmo};
use std::collections::{HashMap, HashSet};

/// O que a linha de fundo já sabe de cada aba aberta.
struct Espelho {
    texto: String,
    revisao: u64,
}

#[derive(Default)]
pub(crate) struct Ligacao {
    espelho: HashMap<PathBuf, Espelho>,
    /// A aba que estava ativa no quadro anterior (trocar de nota grava o que sobrou).
    ativa: Option<PathBuf>,
    /// Até quando uma mudança de arquivo conta como vinda do sync (e não de outro programa).
    pub(crate) sync_ate: Option<Instant>,
}

/// O texto lido do disco é o mesmo que uma aba salvou ou carregou? Então não veio de fora (foi o
/// próprio app que gravou) e não vira "mudou fora do app".
pub(crate) fn aba_ja_tem_o_texto(tabs: &[Tab], card_tabs: &[Tab], path: &Path, text: &str) -> bool {
    tabs.iter()
        .chain(card_tabs)
        .any(|tab| tab.document.path == path && tab.document.saved_text() == Some(text))
}

impl NotesApp {
    /// Uma vez por quadro: sobe o histórico do cofre, entrega o que mudou nas abas e recolhe os
    /// avisos. Nada aqui toca banco, compressão ou hash.
    pub(crate) fn historico_tick(&mut self, ctx: &egui::Context) {
        let Some(vault) = &self.vault else { return };
        if self.historico.is_none() {
            let acordar = ctx.clone();
            self.historico = Some(Historico::iniciar(
                &vault.root,
                self.recovery_config.enabled,
                Ritmo::default(),
                move || acordar.request_repaint(),
            ));
            self.historico_ui.espelho.clear();
            self.historico_ui.ativa = None;
        }
        let Some(historico) = &mut self.historico else {
            return;
        };
        historico.configurar(self.recovery_config.enabled);
        while let Ok(aviso) = historico.avisos.try_recv() {
            self.error = Some(aviso);
        }
        if historico.gravou.try_iter().count() > 0 {
            self.recovery_manager.changed(ctx);
        }
        self.recovery_manager.poll(ctx);
        if !self.recovery_manager.open {
            if let Some(error) = self.recovery_manager.take_error() {
                self.error = Some(error);
            }
            if let Some(notice) = self.recovery_manager.take_notice() {
                self.status = notice;
            }
        }
        if !self.recovery_config.enabled {
            self.historico_ui.espelho.clear();
            self.recovery_saved.clear();
            return;
        }
        let ligacao = &mut self.historico_ui;
        let mut vistos: HashSet<PathBuf> = HashSet::new();
        for tab in self.tabs.iter().chain(self.card_tabs.iter()) {
            let documento = &tab.document;
            // Um arquivo de fora do cofre não cabe no histórico local; imagem e texto puro, também não.
            if documento.is_blank()
                || documento.path.is_absolute()
                || documento.is_image()
                || !rustidian::vault::is_markdown(&documento.path)
            {
                continue;
            }
            // Ainda não carregou do disco.
            let Some(salvo) = documento.saved_text() else {
                continue;
            };
            if !vistos.insert(documento.path.clone()) {
                continue;
            }
            match ligacao.espelho.get_mut(&documento.path) {
                None => {
                    historico.abriu(&documento.path, salvo);
                    ligacao.espelho.insert(
                        documento.path.clone(),
                        Espelho {
                            texto: salvo.to_owned(),
                            revisao: documento.saved_revision(),
                        },
                    );
                }
                Some(espelho) => {
                    let mudou_texto = espelho.texto != documento.text;
                    let mudou_revisao = espelho.revisao != documento.saved_revision();
                    if mudou_revisao && mudou_texto && !documento.dirty() {
                        // Relida do disco (outro programa, sync, restauração): não é digitação.
                        historico.carregou(&documento.path, &documento.text);
                        espelho.texto.clone_from(&documento.text);
                    } else {
                        if mudou_texto {
                            historico.editou(&documento.path, &documento.text);
                            espelho.texto.clone_from(&documento.text);
                        }
                        if mudou_revisao {
                            historico.salvou(&documento.path);
                        }
                    }
                    espelho.revisao = documento.saved_revision();
                }
            }
        }
        // Trocar de aba grava o que sobrou da nota que ficou para trás.
        let ativa = self
            .tabs
            .get(self.active)
            .map(|tab| tab.document.path.clone())
            .filter(|path| ligacao.espelho.contains_key(path));
        if ligacao.ativa != ativa {
            if let Some(anterior) = ligacao.ativa.take()
                && ligacao.espelho.contains_key(&anterior)
            {
                historico.saiu(&anterior);
            }
            ligacao.ativa = ativa;
        }
        // Aba fechada: grava o que sobrou e esquece.
        ligacao.espelho.retain(|path, _| {
            let aberta = vistos.contains(path);
            if !aberta {
                historico.fechou(path);
            }
            aberta
        });
        // O que mudou no disco por fora do app (ou pelo sync) também é uma versão.
        if !self.recovery_saved.is_empty() {
            let origem = if ligacao.sync_ate.is_some_and(|ate| Instant::now() < ate) {
                Origem::Sync
            } else {
                Origem::Externa
            };
            for (path, texto) in self.recovery_saved.drain() {
                historico.gravar(&path, &texto, origem);
            }
        }
    }
}
