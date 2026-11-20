# Quadro H2 (resto) e H3: plano

Ideia vem do Heptabase; o código é nosso. O formato segue JSON Canvas 1.0 puro: o que se grava
a mais são campos opcionais com prefixo `rustidian` e os desconhecidos continuam preservados.

## Peças, arquivos e tipos

Lógica sem tela (módulos próprios, testados em unidade):

| Módulo | Conteúdo |
|---|---|
| `src/canvas_section.rs` (novo) | `headings(text)`, `find_section(text, subpath) -> Section::{Found(range), Missing, NotHeading}`, `replace_range`, `subpath_for(title)`. Cabeçalho ATX fora de blocos de código e fora das propriedades; a seção vai do cabeçalho até o próximo de nível igual ou menor. `#A#B` = B dentro de A. `#^bloco` não é seção (o cartão mostra a nota inteira, como antes). |
| `src/canvas_mini.rs` (novo) | `summarize(canvas, aberto) -> Mini` (cartões como retângulos com título, guarda de ciclo `cycle` quando o cartão aponta para o quadro aberto), `fit(bounds, w, h)`. |
| `src/canvas_layout.rs` (novo) | `align`, `distribute` (extremos fixos, vãos iguais), `equal_spacing` (vão fixo de 40, o primeiro fica), `arrange_grid`, `mind_map` (árvore a partir do cartão, filhos à direita, sem sobreposição, ajusta os lados das setas). Mover um grupo leva o que está dentro dele. |
| `src/canvas.rs` | `Node::tags()` / `Canvas::set_tags` (campo `rustidianTags`), `Canvas::all_tags`, `Canvas::set_subpath`, filtro puro `Filter::matches(node)` (cor e/ou etiqueta; sem acento nem maiúscula). |
| `src/canvas_index.rs` | `Occurrence.subpath` e `Occurrence::label()` ("Quadro · Grupo › Seção"). |
| `src/canvas_notes.rs` | `Notes::board_text(path)` (texto do quadro aberto numa aba ou lido do cofre) para a miniatura. |
| `src/commands.rs` | bloco `// Quadro H2/H3` com os comandos da paleta. |

Tela (`src/canvas_ui.rs`):

- Cartão de seção: o corpo mostra só o trecho; o título vira "Nota › Seção"; editar no cartão
  escreve no `Document` da nota inteira (trecho substituído no lugar, intervalo guardado e
  conferido a cada tecla: se o texto mudou por fora, a edição termina sem tocar em nada).
  Seção sumida: aviso e botão "Mostrar a nota inteira". Menu do cartão de nota: "Mostrar só uma
  seção…" (janela com a lista de cabeçalhos; caixa "Em cartão novo") e "Mostrar a nota inteira".
- Quadro dentro de quadro: o cartão `.canvas` desenha a miniatura (profundidade 1) e abre o
  quadro por duplo clique, Enter ou clique no título; Alt+Seta volta (histórico que já existe).
  Cartão que aponta para o quadro aberto mostra "Quadro já aberto acima".
- Seleção: Shift+arrastar no fundo faz retângulo de seleção (Shift+clique já existia).
  Menu "Organizar" aparece com 2 ou mais cartões selecionados (alinhar, distribuir, espaçamento
  igual, grade) e o menu de um cartão ganha "Mapa mental a partir daqui". Um passo de desfazer
  por comando (um `commit`).
- Filtro: botão "Filtro" na barra abre uma faixa com as cores em uso e as etiquetas; os cartões
  que não casam ficam esmaecidos (e as setas que não ligam dois que casam). Estado só da tela,
  nada vai para o arquivo. Etiquetas: "Etiquetas…" no menu do cartão.

Comandos da paleta (escopo documento, só em aba de quadro): alinhar (6), distribuir na
horizontal e na vertical, espaçamento igual, arrumar em grade, mapa mental a partir do cartão
selecionado, mostrar/esconder o filtro, limpar o filtro, mostrar só uma seção, mostrar a nota
inteira, etiquetar.

## Testes

- Unidade: `canvas_section` (cabeçalhos, código, níveis, `#A#B`, sumido, substituição),
  `canvas_mini`, `canvas_layout` (cada alinhamento, distribuição, grade, árvore sem sobreposição e
  com ciclo), `canvas` (tags ida e volta, filtro), `canvas_index` (subpath), `commands`.
- GUI (`src/app/gui_tests/quadro_h2h3.rs`): seção mostra só o trecho e edita no Document da nota;
  seção sumida avisa e "nota inteira" devolve; escolher a seção pelo menu; quadro dentro de
  quadro (miniatura, abrir, voltar, ciclo); retângulo de seleção; alinhar/distribuir/grade/mapa
  por comando com um desfazer; filtro esmaece sem mexer no arquivo; etiquetas.
- Prints: `print_do_quadro_h2h3` em `gui_tests/prints.rs`.

## Decisões

- Distribuir mantém os dois cartões das pontas e iguala os vãos; "espaçamento igual" fixa o vão
  em 40 unidades ao longo do eixo mais comprido e mantém o primeiro cartão.
- Ponto de contato com `src/app.rs` (fora da fronteira, só ganchos mínimos): despachar os
  comandos novos, esconder no que não é quadro e usar `Occurrence::label()` na linha "Nos
  quadros". Registrado no relatório.
- Arrastar cabeçalho do painel de estrutura não foi feito (o app não tem esse painel no
  quadro); fica o comando/menu "Mostrar só uma seção…".
