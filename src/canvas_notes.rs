//! Notas dentro do quadro (H1): de onde um cartão de nota tira o texto que mostra e onde ele
//! escreve quando o usuário edita no cartão.
//!
//! Regra de ouro: uma nota tem **um** `Document` no app inteiro. Se a nota está numa aba (em
//! qualquer painel), o cartão lê e escreve através dessa aba; se não está, o app guarda um `Tab`
//! sem painel (um "cartão aberto", `NotesApp::card_tabs`) que passa pelo mesmo autosave e pelo
//! mesmo portão do sync de qualquer nota. Nunca há dois escritores para o mesmo arquivo: quando
//! o usuário abre a nota numa aba, o app entrega o texto do cartão à aba e fecha o cartão aberto.

use rustidian::{tab::Tab, vault::Vault};
use std::path::Path;

/// O que um cartão de nota tem para mostrar.
pub enum Content<'a> {
    Text(&'a str),
    /// A nota não existe mais no cofre (apagada ou renomeada fora do app).
    Missing,
}

/// O acesso do quadro às notas do app: as abas (as que vêm antes e as que vêm depois da aba do
/// quadro na lista), os cartões abertos e o cofre.
pub struct Notes<'a> {
    pub vault: Option<&'a Vault>,
    tabs: [&'a mut [Tab]; 2],
    cards: &'a mut Vec<Tab>,
}

impl<'a> Notes<'a> {
    pub fn new(
        vault: Option<&'a Vault>,
        before: &'a mut [Tab],
        after: &'a mut [Tab],
        cards: &'a mut Vec<Tab>,
    ) -> Self {
        Self {
            vault,
            tabs: [before, after],
            cards,
        }
    }

    fn holds(tab: &Tab, path: &Path) -> bool {
        tab.document.path == path && !tab.document.is_blank() && !tab.document.is_image()
    }

    /// A aba (ou o cartão aberto) que guarda a nota. As abas vêm primeiro: com a nota aberta
    /// numa aba, é ela que vale.
    fn find(&self, path: &Path) -> Option<&Tab> {
        self.tabs
            .iter()
            .flat_map(|tabs| tabs.iter())
            .chain(self.cards.iter())
            .find(|tab| Self::holds(tab, path))
    }

    fn find_mut(&mut self, path: &Path) -> Option<&mut Tab> {
        self.tabs
            .iter_mut()
            .flat_map(|tabs| tabs.iter_mut())
            .chain(self.cards.iter_mut())
            .find(|tab| Self::holds(tab, path))
    }

    /// O texto para desenhar: o da aba aberta (que pode ter edição ainda não salva), senão o do
    /// índice do cofre.
    pub fn content(&self, path: &Path) -> Content<'_> {
        if let Some(tab) = self.find(path) {
            return Content::Text(&tab.document.text);
        }
        match self.vault.and_then(|vault| vault.note(path)) {
            Some(note) => Content::Text(&note.text),
            None => Content::Missing,
        }
    }

    /// O texto de um quadro `.canvas` para a miniatura (H2b): o da aba aberta (com a edição ainda
    /// não salva) ou, senão, o do arquivo no cofre. `None` se não existe ou não dá para ler.
    pub fn board_text(&self, path: &Path) -> Option<String> {
        if let Some(tab) = self.find(path) {
            return Some(tab.document.text.clone());
        }
        let vault = self.vault?;
        let absolute = vault.root.join(path);
        let metadata = std::fs::metadata(&absolute).ok()?;
        if metadata.len() > 16 * 1024 * 1024 {
            return None;
        }
        std::fs::read_to_string(absolute).ok()
    }

    /// Se a nota existe (numa aba ou no cofre).
    pub fn exists(&self, path: &Path) -> bool {
        matches!(self.content(path), Content::Text(_))
    }

    /// Se o arquivo pode virar cartão ao soltar no quadro: uma nota do cofre ou uma imagem.
    pub fn accepts(&self, path: &Path) -> bool {
        self.vault.is_some_and(|vault| vault.note(path).is_some())
            || (self.vault.is_some() && rustidian::assets::is_image(path))
    }

    /// A aba que recebe a edição feita no cartão. Sem aba para a nota, abre um cartão aberto
    /// (um `Document` lido do cofre, sem painel nem aba na tela).
    pub fn edit_target(&mut self, path: &Path) -> Result<&mut Tab, String> {
        if self.find(path).is_none() {
            let vault = self.vault.ok_or("Abra um cofre primeiro.")?;
            if vault.note(path).is_none() {
                return Err("Nota não encontrada no cofre.".into());
            }
            let document = vault.open_document(path)?;
            self.cards.push(Tab::new(document));
        }
        self.find_mut(path)
            .ok_or_else(|| "Nota não encontrada no cofre.".into())
    }
}
