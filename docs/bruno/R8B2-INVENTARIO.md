# R8b-2: inventário do que sobra de Python

Gravado antes de migrar qualquer seção (regra da tarefa). Base: `tests/gui_smoke.py` (11.852 linhas, 100 seções em `--section`, sem contar `all`), os demais `.py` listados na seção R8b do PLANO e os workflows.

## Como ler a classificação

- **(a) coberta:** já existe teste Rust que confere o mesmo efeito. Nomeia o teste. Não exige código novo.
- **(b) migrável:** dá para conferir o mesmo estado com `egui_kittest` e os ajudantes de `gui_tests/mod.rs` (`Session::open`, `command`, `open_note`, `type_text`, `key`, `wait_for`). Entra num lote.
- **(b\*) só com gancho:** o efeito que a seção confere só aparece como pixel ou como estado que o app não expõe. Se faltar gancho no app, a seção vira "precisa de gancho no app" e não é mexida agora.
- **(c) depende do sistema:** processo, janela real, área de transferência do SO, seletor nativo, gerenciador de arquivos, `gsettings`, inotify, CPU. Vira teste de integração do crate ou sai, com a cobertura unitária que já existe.
- **pixel:** a seção confere cor, geometria ou medição de pixel. Quando há estado equivalente, a diferença é registrada; quando não há, a parte de pixel fica de fora com motivo escrito.

A classificação é a de primeira passada: vem dos `results.append` de cada função de checagem e dos nomes de testes existentes. Cada lote confere a função real antes de escrever o teste; se a leitura mudar a classe, a linha é corrigida no commit do lote.

## Já migradas em R8b-1 (38 seções)

Testes em `src/app/gui_tests/edicao.rs`, `navegacao.rs` e `historico*`. Continuam no `gui_smoke.py` e nas listas dos workflows: o PLANO pede ver o `cargo test` passar no CI Linux antes de tirar. Ver "Pendências" abaixo.

undo, list-indent, list-enter, delimiter-pairs, unicode-queued, tasks, tables, table-input, table-links, table-edit, folding, inline-styles (parcial), callouts, note-aids, external-rename, block-completion, tab-state, tab-actions, tab-restore, tab-close-menu, navigation, quick-create, quick-ranking, bookmarks, workspaces, note-dialogs, folders, folder-create, file-copy, folder-copy, file-move, trash-options, explorer, palettes, settings, recovery, backlink-context, link-fragments.

## Seções restantes (62)

Coluna "Destino" diz o que acontece nesta fatia. "Lote N" aponta para a seção 4 deste arquivo.

| # | Seção | O que confere (resumo) | Classe | Cobertura existente ou plano | Destino |
|---|---|---|---|---|---|
| 1 | view-modes | Live e Source aceitam edição, Reading rejeita digitação, Ctrl+E restaura o modo | (a) parcial + (b) | `mod.rs::a_visualizacao_de_leitura_nao_aceita_digitacao` e `edita_e_salva_com_ctrl_s...` cobrem Reading e Source. Falta Ctrl+E | Lote 2 |
| 2 | narrow-html | quebra de citação dentro da borda (pixel); cinco formas de `<br>` renderizam sem mudar o fonte | (a) + pixel | `preview.rs::markdown_layout_html_breaks_render_in_paragraphs_quotes_and_table_cells`; `editor_ui.rs::markdown_layout_keeps_wrapped_quote_rows_inside_their_inset`; `editor_ui.rs::markdown_layout_html_breaks_preserve_character_indices_and_source_mode` | sai (a); borda em pixel fica de fora |
| 3 | html-import | Reading de HTML vira Markdown estruturado sem tags de página; nome ocupado é pulado; editar/salvar/desfazer | (b) | `html_import.rs` tem 0 testes; `html_layout.rs` tem 5 (só layout). Comando de importação existe no app | Lote 4 |
| 4 | system-open | abrir arquivo no app padrão do SO, depois editar/salvar/desfazer | (c) | sem teste unitário (nenhuma chamada a `xdg-open`/`ShellExecute` é testada) | integração pendente |
| 5 | system-theme | troca do tema do SO (keyfile do gsettings) redesenha o app sem clique | (c) + (a) | `system_theme.rs` (2 testes: saída do registro, precedência do color-scheme); `theme.rs::system_appearance_changes_both_custom_styles_...` (lógica) | integração pendente; lógica já coberta |
| 6 | graph | o grafo abre pelo fluxo real de cofre; edição, salvar e desfazer antes do comando | (b\*) | `graph.rs` e `graph_ui.rs` com 0 testes; precisa ver se o estado dos nós é exposto | Lote 4 ou gancho |
| 7 | graph-state | arrastar um nó move só o ponto e a aresta; Escape cancela; canvas vazio arrasta; slider | (b\*) + pixel | arrasto real de nó confere pixel | gancho no app (posição do nó) |
| 8 | graph-animation | Animate revela do mais antigo ao mais novo, avança nós pintados, para e termina | (a) + pixel | `graph_animation.rs::playback_reveals_chronologically_and_finishes_without_a_background_clock` | sai (a); nós pintados ficam de fora |
| 9 | sidebar-boundary | sem faixa sem pintura no limite esquerdo (pixel); soltar, arrastar, reiniciar | pixel + (c) | sem estado equivalente exposto; reinício do processo | sai com motivo (pixel e reinício) |
| 10 | toolbar-fill | barra de ferramentas preenche o painel com 0, 1 e muitas abas (geometria) | pixel | sem estado equivalente; geometria | sai com motivo (pixel) |
| 11 | middle-pan | arrastar com botão do meio pelo texto; Escape cancela; retém o painel | (a) | `page_pan.rs` (5 testes): `drag_tracks_release_position...`, `escape_restores_origin_and_disabled_or_changed_document_cancels_capture`, `real_scroll_area_pans_immediately...` | sai (a) |
| 12 | agent-terminal | agentes instalados aparecem; sessão roda na pasta; fecha sem esperar; chaves e cores | (a) + (c) | `terminal.rs` (6): `only_installed_agents_are_offered_in_menu_order`, `a_session_runs_the_agent_in_its_folder...`, `closing_a_session_never_waits_and_still_ends_a_stubborn_agent`; `terminal_ui.rs` (12) | sai com o `tests/agent_terminal_checks.py` (passo 3) |
| 13 | lifecycle | fechar pela janela com agente rodando encerra o processo (código de saída) | (c) | `terminal.rs::closing_a_session_...` cobre a sessão, não o processo | integração pendente |
| 14 | quit | Ctrl+Q pergunta nos dois temas; Escape, Cancelar e clique fora mantêm o app | (b) + pixel | `window.rs` (4): `only_exact_ctrl_q_asks_and_never_closes`, `quit_dialog_closes_once_on_enter_and_never_on_cancel`, `egui_builtin_quit_shortcut_is_off` | Lote 1 (Escape, Cancelar, clique fora) |
| 15 | idle-cpu | CPU ociosa baixa; autosave lê só o próprio arquivo (sem reindexar) | (c) | medição de CPU do processo; sem teste unitário do autosave | sai (medição de processo) |
| 16 | emoji | pixels de emoji colorido no corpo e no nome, nos dois temas | pixel | `emoji_tests.rs` (4): `color_emoji_shape_in_every_family_at_body_and_heading_sizes`, `text_and_emoji_presentation_do_not_poison...`, `emoji_fallback_preserves_...` | sai (a) no nível do rasterizador; captura do app fica de fora |
| 17 | properties | propriedades: alinhamento do cabeçalho (pixel); seleção e desfazer preservam; checkbox e número salvam | (b) + pixel | `properties_ui.rs` sem testes; só o alinhamento é pixel | Lote 3 (parte de estado) |
| 18 | images | imagens wiki e relativas com largura pedida (pixel); edição externa atualiza sem mudar a nota; embed mostra o fonte | (b) + pixel | `preview.rs::native_images_preserve_caption_and_neighboring_text` (parcial); `image_view.rs` sem testes | Lote 4 (parte de estado) |
| 19 | attachments | Explorer abre aba de imagem; zoom ajustar e 100%; atalhos não mudam bytes; completar link mantém extensão; Ctrl+Enter e voltar | (b) + (a) | Ctrl+Enter e voltar/avançar em `navegacao.rs::seguir_ligacoes_wiki_e_markdown_e_voltar_avancar_com_alt_setas` e `ctrl_enter_sobre_uma_ligacao_...` | Lote 4 (aba de imagem e zoom) |
| 20 | large-search | 550 resultados, todos alcançáveis; a última nota abre | (b) | `mod.rs::busca_nas_notas_pelo_painel_lateral` cobre o caminho; falta o cofre grande | Lote 3 |
| 21 | instance | segunda abertura foca a janela existente; outro cofre ganha janela própria | (a) | `instance.rs` (13): `a_launch_exits_once_the_window_has_the_focus`, `a_request_for_another_vault_gets_a_window_of_its_own...`, `a_launch_beside_a_silent_window_takes_its_socket` | sai (a) |
| 22 | speed | nota de 86 KB com 375 blocos de código: tempo medido | (c) medição | `perf.rs` sem testes; medição de tempo não é teste de GUI | sai (medição; vira bench se o xtask pedir) |
| 23 | watch-limit | pasta externa acrescenta um vigia nativo sob limite pequeno; CPU ociosa | (c) | inotify do Linux; `watcher.rs` sem testes | sai (Linux) |
| 24 | explorer-multi | Ctrl+clique seleciona duas entradas sem abrir; mover em massa salva rascunhos, mantém abas, preserva bytes de colisão | (b) | `explorer_selection.rs` sem testes; `navegacao.rs` tem `file-move`/`folder-copy` (migrados) | Lote 3 |
| 25 | reveal | gerenciador de arquivos real abre e seleciona o item; URI da área de transferência | (c) | depende do gerenciador do SO e da área de transferência | integração pendente |
| 26 | vault-switch | cofre que não abre mantém a nota; nomes iguais em cofres diferentes não dividem desfazer; seletor nativo | (b) + (c) | `vaults.rs::separate_windows_preserve_each_others_vaults_and_removals` (lógica); seletor nativo é (c) | Lote 3 (lógica) |
| 27 | vaults | perfil de cofres abre o seletor nos dois temas; cancelar criação mantém o cofre ativo e não cria nada | (a) + (b) | `vaults.rs::creating_a_vault_is_contained_empty_and_never_reuses_a_destination` (criação); seletor é (b) | Lote 3 (seletor) |
| 28 | desktop-vault | área de trabalho como cofre principal, launcher `.desktop`, carregamento cancelável | (c) | `vaults.rs::the_desktop_and_the_main_vault_stay_on_top...` (lógica); `.desktop` é Linux | integração pendente; usa `tests/vault_loading_checks.py` |
| 29 | file-toolbar | ícone de nova pasta na barra cria pasta vazia | (b) | `folder-create` (migrada) cobre criação pelo comando; o ícone da barra não | Lote 1 |
| 30 | typography | citação do Live com barras pintadas; código literal; sublinhado Setext visível | pixel + (a) | `preview.rs`, `editor_ui.rs` (layout e fonte) | sai o fonte (a); barras e tamanho ficam de fora |
| 31 | font-controls | comandos de fonte mudam o glifo e as bordas de leitura em um pixel; bytes não mudam | (a) + pixel | `theme.rs::document_font_controls_change_glyphs_without_resizing_other_ui` | sai (a); um pixel fica de fora |
| 32 | font-families | DejaVu Serif instalada como fonte de texto; código idêntico em pixel | (c) + (a) | `theme.rs::prose_and_heading_families_cover_persian_letters`; fonte instalada depende do SO | sai (a); fonte do SO é (c) |
| 33 | visual-controls | pontos do grafo e cursores dos sliders compactos (pixel) | pixel | sem estado equivalente | sai com motivo (pixel) |
| 34 | code | bloco de código inativo em painel de largura cheia (pixel); altura de linha medida | pixel + (a) | `preview.rs::reader_preserves_code_body_bytes_empty_blocks_and_literal_examples` | sai (a); geometria fica de fora |
| 35 | tab-overflow | dez abas: a ativa fica inteira ao voltar pela barra com rolagem | pixel / (b\*) | rolagem da barra de abas não é exposta | gancho no app (estado de rolagem) |
| 36 | tab-drag | setas, Home e End navegam a barra de abas com foco; setas no editor mantêm o documento | (b) | `navegacao.rs` cobre ciclar abas (`comandos_de_aba_ciclam_criam_e_fecham...`); falta o foco na barra | Lote 1 (teclado); arrastar com mouse fica de fora |
| 37 | image-import | colar PNG da área de transferência grava os pixels na raiz do cofre | (c) | depende da área de transferência do SO | integração pendente |
| 38 | syntax | cores de tokens de linguagem (pixel); idioma exibido; bytes exatos | (a) + pixel | `syntax.rs` (3): `reading_and_editing_keep_their_measured_function_and_primitive_colors`, `rust_colors_follow_the_reference...`, `language_files_and_fences_color_real_tokens_without_changing_text` | sai (a) com o passo do `syntax.yml` |
| 39 | sidebar-keys | marcadores na ordem salva; menus e diálogos com setas (pixel); bytes intactos | (a) + pixel | `navegacao.rs` (favoritos, `titulo_do_favorito_...`, `renomear_arquivo_e_pasta_...`) | sai (a); setas ficam de fora |
| 40 | tree-scroll | abrir nota fundo na árvore rola o explorador até a linha | (b\*) | rolagem do explorador não é exposta | gancho no app (estado de rolagem) |
| 41 | context-menu | botão direito no editor abre menu com Recortar, Copiar, Colar, Selecionar tudo, formatação, Título 1 a 3 | (b) | `edicao.rs` tem `right_click_label`; falta o teste | Lote 2 |
| 42 | keyboard-focus | comando de modo indisponível em aba em branco; disponível em nota real | (a) | `commands.rs::native_color_toggles_require_an_actual_document_editor`, `tab_actions_need_a_tab...`; `nota_quadro.rs::a_paleta_nao_oferece_ir_para_o_modo_em_que_ja_esta` | sai (a) |
| 43 | reading-code | leitura: código em painel de largura cheia; cercas escondidas | (a) + pixel | `preview.rs::reader_preserves_code_body_bytes_empty_blocks_and_literal_examples` | sai (a); largura fica de fora |
| 44 | table-resize | arrastar divisória de coluna; um desfazer e refazer restauram larguras exatas | (b\*) | arrasto de mouse; a R8b-1 deixou arrasto de mouse de fora | Lote 2, se o harness arrastar; senão gancho |
| 45 | table-edges | hover revela tiras `+` nas bordas; clique acrescenta uma linha ou coluna; digitar aceita | (b) + pixel | clique numa tira é estado; hover é pixel | Lote 2 (clique) |
| 46 | table-select | seleção de linhas e colunas da tabela (rótulos) | (b\*) | confirmar no lote quais rótulos existem | Lote 2 se der |
| 47 | chrome-spacing | barra de busca e linhas seguem a escala de espaçamento (geometria) | pixel | geometria; sem estado | sai com motivo (pixel) |
| 48 | persian | digitação, copiar e colar, seleção, apagar e desfazer em persa; somente leitura | (a) | `edicao.rs::texto_persa_grego_e_zwnj_enfileirado_grava_bytes_exatos`; `theme.rs::bidi_*` | sai (a) |
| 49 | essential | destaques nativos em Live, Reading e Source; código literal; inserir pela paleta preserva seleção e desfazer; busca difusa | (a) + (b) | `commands.rs::fuzzy_commands_find_abbreviations_and_exclude_image_edits`, `exact_portable_and_native_highlight_names_win...`; inserção pela paleta em `navegacao.rs::palettes` (migrada) | Lote 2 (o que faltar) |
| 50 | colors | cinco cores de destaque: sobrevivem a Ctrl+P, salvar e reabrir, desfazer; captura nos temas | (b) + pixel | `commands.rs::native_color_toggles_require_an_actual_document_editor` (disponibilidade) | Lote 2 (estado) |
| 51 | native-highlights | cores indisponíveis em aba em branco; Cyan original em documento sem editar | (a) | `commands.rs::native_color_toggles_require_an_actual_document_editor`; `commands.rs::exact_portable_and_native_highlight_names_win...` | sai (a) |
| 52 | page-preview | prévia ao passar sobre link de leitura; coordenadas do link levam ao alvo | pixel / (b\*) | `page_preview.rs` sem testes; precisa de gancho para o estado da prévia | gancho no app |
| 53 | hover | hover muda ícones, fechar aba, pastas e arquivos (pixel) | pixel | sem estado equivalente | sai com motivo (pixel) |
| 54 | title | título inline renomeia com Enter ou ao sair; atualiza links e abas; Escape cancela; não sobrescreve | (b) | `title_ui.rs` sem testes; renomear pelo app é coberto em `navegacao.rs` (fora do título) | Lote 1 |
| 55 | scrolling | leitura e Live rolam 300 px por entalhe, sem cauda de suavização | (a) | `theme.rs::wheel_steps_are_immediate_and_leave_no_smoothing_tail` | sai (a) |
| 56 | diagrams | tabelas e Mermaid renderizam em Reading e Live; literais em Source; sintaxe inválida mantém o texto | (b) + pixel | `diagrams.rs` sem testes; a parte de texto é estado | Lote 2 (estado) |
| 57 | math | glifos de matemática pintados (pixel); seguem a fonte; janela estreita; bytes | (a) + pixel | `math.rs` (4) e `preview.rs::reading_math_keeps_surrounding_prose...` | sai (a); glifos ficam de fora |
| 58 | chrome | layout do cromo (pixel) | pixel | geometria; sem estado | sai com motivo (pixel) |
| 59 | note-creators | configurações de nota única persistem pasta, formato e modelo; visíveis nos dois temas | (b) | sem teste; `note-aids` (migrada) cobre nota diária | Lote 1 |
| 60 | note-embeds | nota embutida renderiza; clique em tarefa salva só a nota referida; ligações resolvem na nota de origem | (b) | sem teste | Lote 2 |
| 61 | drag-move | arrastar nota sobre pasta recolhida abre a pasta após N s; roda do mouse durante o arrasto | (c) | arrasto de mouse real e tempo; a R8b-1 deixou de fora | sai com motivo (mouse real) |
| 62 | palette-queued | Escape enfileirado fecha Configurações sem fechar a paleta de substituição nem inserir o texto | (b) | `edicao.rs::queued()` e `unicode-queued` (migrada) dão o mecanismo | Lote 1 |

Contagem dos 62 restantes, pelo destino da fatia:

- **Migrar para egui_kittest: 22** (Lote 1: 6 · Lote 2: 7 · Lote 3: 5 · Lote 4: 4), mais 2 de Lote 2 se o harness conseguir arrastar (table-resize, table-select).
- **Sai porque a cobertura unitária existe (a): 18** (narrow-html, graph-animation, middle-pan, agent-terminal, emoji, instance, typography, font-controls, font-families, code, syntax, sidebar-keys, keyboard-focus, reading-code, persian, native-highlights, scrolling, math). Onde a seção também conferia pixel, a parte de pixel fica de fora e está escrita na linha.
- **Sai por ser pixel ou geometria sem estado equivalente: 6** (sidebar-boundary, toolbar-fill, visual-controls, chrome-spacing, hover, chrome).
- **Sai ou vira integração, por depender do sistema (c): 10** (system-open, system-theme, lifecycle, idle-cpu, speed, watch-limit, reveal, desktop-vault, image-import, drag-move).
- **Precisa de gancho no app: 4** (graph-state, tab-overflow, tree-scroll, page-preview); table-resize e table-select entram aqui se o harness não arrastar.

Total: 22 + 18 + 6 + 10 + 4 + 2 (table-resize e table-select, cada um migrado ou gancho) = 62.

## Outros arquivos `.py` (passo 3)

| Arquivo | Decisão | Motivo |
|---|---|---|
| `experiments/html-host/verify.py` | **apagar** (neste PR, com o README do experimento atualizado) | experimento CEF abandonado (PLANO); sem teste Rust equivalente, e o próprio experimento não é mais usado |
| `scripts/compare-editors.py` | **apagar** (passo do lote final; depende de `native_input.py` e `record-keyboard.py`) | compara com o editor Ferrite por xdotool; não mede o Rustidian sem X11 e não tem equivalente Rust. Pendência: `README.md:146` linka este script |
| `scripts/record-keyboard.py` | apagar junto com `compare-editors.py` | só o `compare-editors.py` o chama |
| `scripts/native_input.py` | apagar junto com o último uso | ajudante do X11 usado por `gui_smoke.py` e `compare-editors.py` |
| `tests/vault_loading_checks.py` | sai com a seção `desktop-vault` (c) | módulo do `gui_smoke`; hoje fica |
| `tests/agent_terminal_checks.py` | sai com a seção `agent-terminal` (a) | módulo do `gui_smoke`; cobertura em `terminal.rs` e `terminal_ui.rs` |
| `tests/emoji_font_asset.py` | conferências de hash viram teste Rust no lote final; a de shaping HarfBuzz **sai com motivo** | a parte de HarfBuzz não tem equivalente no crate (shaping passa pelo `epaint`) |

## Workflows (passo 4)

Não mexi neles nesta fatia. `identity.yml`, `ux.yml` e `syntax.yml` chamam `python3 tests/gui_smoke.py` com listas de seções, e ainda há heredocs Python inline (`python3 - <<'PY'`). Como o `gui_smoke.py` continua com 62 seções sem equivalente Rust, trocar tudo por `cargo test` agora tiraria a cobertura dessas seções do CI. A troca vem junto com o último `--section` que sair do Python. Ver "Pendências".

## Lotes

1. **Lote 1 (`migrados_janela.rs`):** quit (Escape, Cancelar, clique fora), palette-queued, tab-drag (teclado), file-toolbar, title, note-creators.
2. **Lote 2 (`migrados_editor.rs`):** view-modes (Ctrl+E), context-menu, note-embeds, table-edges (clique), colors (estado), diagrams (estado), essential (o que faltar).
3. **Lote 3 (`migrados_explorador.rs`):** explorer-multi, large-search, vault-switch (lógica), vaults (seletor), properties (estado).
4. **Lote 4 (`migrados_grafo_imagens.rs`):** html-import, graph (se o estado for exposto), images (estado), attachments (aba de imagem e zoom).

Sem gancho no app: graph-state, tab-overflow, tree-scroll, page-preview, table-resize (arrasto), table-select. Ficam em "precisa de gancho no app" até um dono decidir.

## Pendências

- Os 38 de R8b-1 seguem no `gui_smoke.py` e nas listas dos workflows, como pede o PLANO ("primeiro ver o `cargo test` passar no CI Linux"). Não há CI Linux aqui.
- O `xtask/src/release.rs` (linhas 735–745) lista `tests/gui_smoke.py`, `scripts/compare-editors.py`, `scripts/record-keyboard.py` e `scripts/native_input.py` em `reuse_allowed`. Só é lista de texto, não quebra compilação. Fora da fronteira desta tarefa.
- `README.md:146` linka `scripts/compare-editors.py`. Fora da fronteira.
- `.github/workflows/store-smoke.yml` usa `xvfb-run` para o smoke do snap, não chama Python. Não entra no passo 4.
