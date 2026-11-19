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
        if self.dialog != Some(Dialog::Settings)
            || self.settings_page != settings_ui::Page::Recovery
        {
            self.espaco.fechada();
        }
        let Some(vault) = &self.vault else { return };
        if self.historico.is_none() {
            let acordar = ctx.clone();
            self.historico = Some(Historico::iniciar(
                &vault.root,
                self.recovery_config.enabled,
                self.recovery_config.politica(),
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
        historico.ajustar_politica(self.recovery_config.politica());
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

/// Número com ponto de milhar: 1234 vira "1.234".
pub(crate) fn milhar(numero: usize) -> String {
    let digitos = numero.to_string();
    let mut saida = String::new();
    for (i, c) in digitos.chars().enumerate() {
        if i > 0 && (digitos.len() - i) % 3 == 0 {
            saida.push('.');
        }
        saida.push(c);
    }
    saida
}

/// "380 B", "380 KB", "1,2 MB": o tamanho em disco como a pessoa lê.
pub(crate) fn tamanho_legivel(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", (bytes as f64 / 1024.0).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)).replace('.', ",")
    }
}

/// O que a página "Recuperação de arquivos" sabe do espaço do histórico. Nada aqui espera a linha
/// de fundo: só `try_recv` a cada quadro.
#[derive(Default)]
pub(crate) struct EspacoDoHistorico {
    numeros: Option<rustidian::historico_fundo::Estatisticas>,
    esperando:
        Option<std::sync::mpsc::Receiver<Result<rustidian::historico_fundo::Estatisticas, String>>>,
    compactando: Option<
        std::sync::mpsc::Receiver<Result<rustidian::historico_fundo::RelatorioCompactacao, String>>,
    >,
    /// Aviso do fim da compactação, ou a mensagem de erro.
    pub(crate) aviso: Option<String>,
    /// A página já pediu os números nesta abertura.
    pediu: bool,
    /// A gerência do histórico (limpar, apagar) estava ocupada no quadro anterior.
    gerencia_ativa: bool,
}

impl EspacoDoHistorico {
    /// A página saiu da tela: a próxima abertura pede os números de novo.
    pub(crate) fn fechada(&mut self) {
        self.pediu = false;
    }

    pub(crate) fn calculando(&self) -> bool {
        self.esperando.is_some() || (self.numeros.is_none() && self.aviso.is_none())
    }

    pub(crate) fn compactando(&self) -> bool {
        self.compactando.is_some()
    }

    /// O rótulo da linha "Espaço".
    pub(crate) fn rotulo(&self, ligado: bool) -> String {
        if !ligado {
            return "Histórico desligado".into();
        }
        if let Some(erro) = &self.aviso
            && self.numeros.is_none()
        {
            return erro.clone();
        }
        match self.numeros {
            Some(n) => format!(
                "{} {} de {} {} · {} em disco",
                milhar(n.versoes),
                if n.versoes == 1 {
                    "versão"
                } else {
                    "versões"
                },
                milhar(n.notas),
                if n.notas == 1 { "nota" } else { "notas" },
                tamanho_legivel(n.tamanho_em_disco)
            ),
            None => "Calculando…".into(),
        }
    }

    fn pedir_numeros(&mut self, historico: &Historico) {
        match historico.estatisticas() {
            Ok(rx) => self.esperando = Some(rx),
            Err(erro) => self.aviso = Some(erro),
        }
    }

    pub(crate) fn compactar(&mut self, historico: &Historico) {
        match historico.compactar() {
            Ok(rx) => {
                self.compactando = Some(rx);
                self.aviso = None;
            }
            Err(erro) => self.aviso = Some(erro),
        }
    }

    /// Chamado a cada quadro em que a página aparece: pede os números na primeira vez (e depois
    /// de limpar), recolhe respostas e pede repaint enquanto algo espera.
    pub(crate) fn quadro(
        &mut self,
        ctx: &egui::Context,
        historico: Option<&Historico>,
        ligado: bool,
        gerencia_ativa: bool,
    ) {
        let nova_abertura = !self.pediu;
        self.pediu = true;
        let Some(historico) = historico.filter(|_| ligado) else {
            return;
        };
        let acabou_de_limpar = self.gerencia_ativa && !gerencia_ativa;
        self.gerencia_ativa = gerencia_ativa;
        if nova_abertura {
            self.numeros = None;
            self.aviso = None;
        }
        if (nova_abertura || acabou_de_limpar) && self.esperando.is_none() {
            self.pedir_numeros(historico);
        }
        if let Some(rx) = &self.esperando
            && let Ok(resposta) = rx.try_recv()
        {
            self.esperando = None;
            match resposta {
                Ok(n) => self.numeros = Some(n),
                Err(erro) => self.aviso = Some(erro),
            }
        }
        if let Some(rx) = &self.compactando
            && let Ok(resposta) = rx.try_recv()
        {
            self.compactando = None;
            match resposta {
                Ok(r) => {
                    self.aviso = Some(format!(
                        "Compactado: {} → {}",
                        tamanho_legivel(r.bytes_antes),
                        tamanho_legivel(r.bytes_depois)
                    ));
                    self.pedir_numeros(historico);
                }
                Err(erro) => self.aviso = Some(erro),
            }
        }
        if self.esperando.is_some() || self.compactando.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod testes_do_espaco {
    use super::*;

    #[test]
    fn milhar_e_tamanho() {
        assert_eq!(milhar(0), "0");
        assert_eq!(milhar(999), "999");
        assert_eq!(milhar(1234), "1.234");
        assert_eq!(milhar(1_234_567), "1.234.567");
        let um = EspacoDoHistorico {
            numeros: Some(rustidian::historico_fundo::Estatisticas {
                notas: 1,
                versoes: 1,
                tamanho_em_disco: 2048,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(um.rotulo(true), "1 versão de 1 nota · 2 KB em disco");
        assert_eq!(um.rotulo(false), "Histórico desligado");
        assert_eq!(tamanho_legivel(512), "512 B");
        assert_eq!(tamanho_legivel(380 * 1024), "380 KB");
        assert_eq!(tamanho_legivel(1_258_291), "1,2 MB");
    }
}
