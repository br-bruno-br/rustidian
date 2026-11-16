//! Portuguese (Brazil) text for labels whose English wording is also an identity: the screen test
//! finds those controls by `button:<English tooltip>` / `menu:<English label>` probe ids, so the
//! source keeps the English text and this table gives the text shown on screen.

/// The text shown for an English label; unknown labels are shown unchanged.
pub fn t(english: &str) -> &str {
    match english {
        // Barra de ferramentas e atalhos
        "Toggle left sidebar" => "Alternar painel lateral esquerdo",
        "Toggle right sidebar" => "Alternar painel lateral direito",
        "New note · Ctrl+N" => "Nova nota · Ctrl+N",
        "Open graph view" => "Abrir visão de grafo",
        "Open today's daily note" => "Abrir a nota diária de hoje",
        "Open local history" => "Abrir histórico local",
        "Insert template" => "Inserir modelo",
        "Command palette · Ctrl+P" => "Paleta de comandos · Ctrl+P",
        "File explorer" => "Explorador de arquivos",
        "Search notes" => "Buscar notas",
        "Bookmarks" => "Favoritos",
        "New folder" => "Nova pasta",
        "Change sort order" => "Alterar ordem de classificação",
        "Collapse all folders" => "Recolher todas as pastas",
        "Open settings · Ctrl+," => "Abrir configurações · Ctrl+,",
        "Navigate back · Alt+Left" => "Voltar · Alt+Esquerda",
        "Navigate forward · Alt+Right" => "Avançar · Alt+Direita",
        "More options" => "Mais opções",
        "Current view: reading\nClick to edit · Ctrl+E" => {
            "Modo atual: leitura\nClique para editar · Ctrl+E"
        }
        "Current view: editing\nClick to read · Ctrl+E" => {
            "Modo atual: edição\nClique para ler · Ctrl+E"
        }
        "New tab · Ctrl+T" => "Nova aba · Ctrl+T",
        "Dismiss" => "Dispensar",
        "Close local graph" => "Fechar grafo local",
        "Delete group" => "Excluir grupo",
        "Close" => "Fechar",
        "Restart" => "Reiniciar",
        "Delete layout" => "Excluir layout",
        // Menus
        "Rename or move…" => "Renomear ou mover…",
        "Delete file" => "Excluir arquivo",
        "Close others" => "Fechar as outras",
        "Close tabs to the right" => "Fechar abas à direita",
        "Unpin" => "Desafixar",
        "Pin" => "Fixar",
        "Copy path" => "Copiar caminho",
        "Copy file name" => "Copiar nome do arquivo",
        "Make a copy" => "Fazer uma cópia",
        "Reveal in system file manager" => "Mostrar no gerenciador de arquivos",
        "Delete" => "Excluir",
        "Open in new tab" => "Abrir em nova aba",
        "Move file to..." => "Mover arquivo para...",
        "Bookmark…" => "Favoritar…",
        "Rename…" => "Renomear…",
        // Seletores de modo
        "Reading" => "Leitura",
        "Live preview" => "Pré-visualização ao vivo",
        "Live Preview" => "Pré-visualização ao vivo",
        "Source mode" => "Modo código-fonte",
        "Split view" => "Visão dividida",
        "Graph" => "Grafo",
        "Fit" => "Ajustar",
        "Close window" => "Fechar janela",
        "Restore window" => "Restaurar janela",
        "Maximize window" => "Maximizar janela",
        "Minimize window" => "Minimizar janela",
        // Tabelas
        "Insert row above" => "Inserir linha acima",
        "Insert row below" => "Inserir linha abaixo",
        "Delete row" => "Excluir linha",
        "Move row up" => "Mover linha para cima",
        "Move row down" => "Mover linha para baixo",
        "Insert column left" => "Inserir coluna à esquerda",
        "Insert column right" => "Inserir coluna à direita",
        "Delete column" => "Excluir coluna",
        "Delete columns" => "Excluir colunas",
        "Move column left" => "Mover coluna para a esquerda",
        "Move column right" => "Mover coluna para a direita",
        "Align left" => "Alinhar à esquerda",
        "Align center" => "Centralizar",
        "Align right" => "Alinhar à direita",
        "Sort ascending" => "Ordem crescente",
        "Sort descending" => "Ordem decrescente",
        "Clear contents" => "Limpar conteúdo",
        "Filters" => "Filtros",
        "Groups" => "Grupos",
        "Display" => "Exibição",
        "Forces" => "Forças",
        "Link direction" => "Direção dos links",
        "Tags" => "Tags",
        // Menu de edição
        "Cut" => "Recortar",
        "Copy" => "Copiar",
        "Paste" => "Colar",
        "Select all" => "Selecionar tudo",
        "Bold" => "Negrito",
        "Italic" => "Itálico",
        "Strikethrough" => "Tachado",
        "Highlight" => "Destaque",
        "Code" => "Código",
        "Link" => "Link",
        "Heading 1" => "Título 1",
        "Heading 2" => "Título 2",
        "Heading 3" => "Título 3",
        "Search for selection" => "Buscar a seleção",
        "Default" => "Padrão",
        "System" => "Sistema",
        "Dark" => "Escuro",
        "Light" => "Claro",
        "Editing view" => "Modo de edição",
        "Reading view" => "Modo de leitura",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::t;

    #[test]
    fn unknown_labels_are_shown_unchanged() {
        assert_eq!(t("Nothing to translate"), "Nothing to translate");
        assert_eq!(t("Close"), "Fechar");
    }
}
