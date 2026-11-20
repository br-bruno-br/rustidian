# Histórico completo das notas — briefing

Pedido do Bruno em 18/11/2026: guardar o histórico inteiro de cada nota (centenas ou milhares de versões), ocupando pouco espaço, abrindo qualquer versão na hora, com "antes e depois" e um desfazer que vai além do Ctrl+Z do Word. Este arquivo é só o plano. Nada está implementado.

## É possível?

Sim. Texto é o melhor caso para compressão: de uma versão para a outra muda pouca coisa.

Conta para uma nota de 5 KB editada 1.000 vezes, mudando uns 50 caracteres por vez:

| Jeito de guardar | Espaço para 1.000 versões |
|---|---|
| Cópia inteira de cada versão | ~5 MB |
| Cópia inteira comprimida (zstd) | ~1,5 MB |
| Só a diferença de cada versão, comprimida, com uma cópia cheia a cada 64 | **~100 a 200 KB** |

Para abrir qualquer versão, basta pegar a cópia cheia mais próxima e aplicar no máximo 63 diferenças. Isso leva menos de 5 ms.

O que já temos no projeto e serve aqui:
- `redb`, um banco embutido em Rust, que já é dependência;
- `blake3` para impressão digital, já usado no `nota-sync`;
- a criptografia do `nota-cripto`.

O Ctrl+Z de hoje vive só na memória: cada desfazer volta um grupo de digitação, e tudo se perde quando o aplicativo fecha.

## Técnicas possíveis

1. **Cópia cheia + diferenças (como o Git e o xdelta).** De tempos em tempos uma cópia inteira (o "quadro-chave"), e entre elas só as diferenças. Variante esperta: a versão mais nova fica sempre inteira e as antigas viram diferenças "para trás", como o antigo RCS. Assim abrir a nota de hoje é instantâneo e só a viagem ao passado paga a reconstrução.
2. **zstd usando a versão anterior como dicionário** (o modo "patch-from" do zstd). Comprime a versão nova "sabendo" da anterior. Na prática vira diferença e compressão de uma vez só, sem algoritmo de diff próprio. Descomprime a mais de 1 GB/s. É a peça mais simples de maior ganho.
3. **Pedaços com endereço pelo conteúdo** (blake3 + FastCDC, como o restic/rustic). O texto vira blocos identificados pela impressão digital, e o bloco repetido é guardado uma vez só. Isso vale entre versões e até entre notas (parágrafo copiado, nota duplicada). Ganha mais em notas grandes.
4. **Dicionário treinado do cofre.** O zstd treina um dicionário com as próprias notas (o jeito de escrever, o Markdown, as palavras repetidas). Isso melhora muito a compressão de versões pequenas.
5. **Registro de cada operação, letra a letra (CRDT).** Guarda cada inserção e apagamento, como o "controlar alterações" do Word. As bibliotecas em Rust são `diamond-types` (a mais compacta: poucos bytes por tecla), `yrs` (Yjs) e `automerge`. Dá para rever a escrita tecla por tecla e, de brinde, juntar edições de dois aparelhos sem conflito no sync. É mais trabalho e muda a base do editor, então fica para depois.
6. **Árvore de desfazer (como o undotree do Vim).** No Word, se você desfaz três passos e digita algo, os três passos somem para sempre. Na árvore eles viram um galho, e dá para voltar a qualquer um. A árvore também fica gravada e sobrevive ao fechar o aplicativo.

## Quando gravar uma versão (granularidade)

- **Na memória, para o Ctrl+Z:** como hoje, por grupo de digitação.
- **No disco, como versão do histórico:**
  - depois de 2 s sem digitar;
  - ao salvar, trocar de nota ou fechar;
  - a cada ~300 caracteres digitados sem pausa;
  - ao renomear ou mover a nota.
- **Mudança de fora** (outro programa ou o sync): também vira versão, marcada com a origem ("veio do celular", "mudou fora do aplicativo").
- **Gravação fora da tela:** numa linha separada, sem travar a digitação; perde-se no máximo 1 s se cair a energia.

## Guardar sem crescer para sempre (retenção)

Como o Time Machine do Mac:
- tudo das últimas 24 h;
- 1 versão por hora na semana;
- 1 por dia no mês;
- 1 por semana para sempre.

Regras que acompanham:
- **Versões com nome** ("enviado ao editor", "rascunho 2") nunca são apagadas.
- **Compactação** de tempos em tempos: junta diferenças e refaz quadros-chave.
- **Teto por nota e por cofre**, configurável.
- **"Apagar do histórico":** uma senha ou um dado colado por engano precisa poder sumir de todas as versões.

## Ideias para quem escreve

- **Máquina do tempo da nota:** uma barra deslizante. Arrasta e vê o texto daquele momento, com o que entrou em verde e o que saiu em vermelho, como o histórico do Google Docs.
- **Diferença por palavra, não por linha:** em texto corrido uma linha é um parágrafo inteiro, então comparar por palavra mostra o que mudou de fato. Bibliotecas: `similar` (Myers/patience) ou `imara-diff`.
- **Histórico de um trecho:** seleciona um parágrafo e vê como ele nasceu e mudou, e restaura só ele sem mexer no resto.
- **Achar o que apaguei:** busca em todo texto que já saiu de qualquer versão ("eu escrevi uma frase sobre X semana passada").
- **Variantes:** "experimentar outra versão deste capítulo". Cria um galho, escreve, compara lado a lado e escolhe.
- **Painel do escritor:** palavras escritas por dia, sequência de dias, palavras cortadas e mapa de calor. Tudo sai do histórico, sem custo extra.
- **Replay da escrita:** a nota se escrevendo sozinha, acelerada. É útil para rever o raciocínio e é divertido.
- **Ctrl+Z que sobrevive:** fechou o aplicativo, abriu amanhã, e o desfazer continua lá.
- **Antes e depois lado a lado:** duas versões quaisquer, ou duas notas diferentes.

## Recomendação por fases

| Fase | O que entra | Tamanho |
|---|---|---|
| **H1 Cofre de versões** | `redb` com as versões de cada nota: zstd com a anterior como dicionário, quadro-chave a cada 64, impressão digital blake3 por versão; gravação na pausa, ao salvar e ao fechar; lista de versões, restaurar e ver a diferença | pequena, maior ganho |
| **H2 Máquina do tempo** | barra deslizante, diferença por palavra, versões com nome, retenção, compactação e "apagar do histórico" | média |
| **H3 Desfazer que fica** | árvore de desfazer gravada no `redb`, com galhos e painel para escolher o galho | média |
| **H4 Escritor** | histórico de trecho, achar o que apaguei, variantes, painel e replay | média, por partes |
| **H5 Letra a letra (opcional)** | `diamond-types` ou `yrs` como registro de operações; junta edições de dois aparelhos no sync | grande; só se o sync começar a dar conflito |

## Régua para dar como pronto (H1)

Na medição, uma nota sintética de 5 KB com 1.000 edições:
- **Espaço:** abaixo de 200 KB.
- **Abrir:** qualquer versão em menos de 5 ms.
- **Gravar:** uma versão em menos de 1 ms, sem travar a tela.

Teste de propriedade: reconstruir **cada** versão e comparar com a impressão digital blake3 gravada, com zero diferenças. É o teste diferencial que já usamos em outros lugares.

## Armadilhas conhecidas

- **Identidade da nota:** o histórico precisa seguir a nota quando ela é renomeada ou movida. Usar um id estável, não o caminho (o renomear já atualiza os quadros, mesma ideia).
- **O arquivo `.md` continua sendo a verdade:** o histórico é um anexo (por exemplo `.rustidian/historico.redb` no cofre). Se o histórico sumir, a nota não se perde.
- **Segredos:** o histórico guarda o que foi apagado. No cofre sincronizado, ele vai cifrado (`nota-cripto`), e existe o "apagar do histórico".
- **Android:** a crate `zstd` compila C (zstd-sys). Funciona com o NDK, mas é mais uma coisa no build. Alternativas em Rust puro: `lz4_flex` (mais rápido, comprime menos) ou `ruzstd`. Medir os dois.
- **Sync:** na H1 o histórico é local de cada aparelho. Sincronizar o histórico fica para depois da H2.
- **Nunca gravar na linha da tela:** a gravação e a compactação vão numa linha de fundo.

## H2 Máquina do tempo: o que foi feito (19/11/2026)

Ramo `h2-maquina-tempo`, partindo da H1 (0ef6ed5). Tudo em pt-BR e sem gravar tecla a tecla: a versão continua nascendo no fim da frase, na pausa, ao salvar e ao trocar de nota (o `Agrupador` da H1).

**Na tela de versões** (`src/recovery_ui.rs`):
- **Linha do tempo:** uma barra deslizante da versão mais antiga (esquerda) à mais nova (direita), com ◀ e ▶. Arrastar mostra o texto daquele momento com o que entrou em verde e o que saiu em vermelho, por palavra (contra a versão anterior, ou contra o texto atual se "Comparar com o texto atual" estiver ligado). A lista da esquerda acompanha. A barra lê uma versão por vez e a última posição vence: arrastar rápido não trava nem faz o botão voltar.
- **Versões com nome:** "Dar nome" / "Mudar nome" / "Tirar o nome" (até 80 caracteres). A versão com nome aparece em destaque na lista, ganha um tracinho na barra e nunca sai do histórico. O botão "Só com nome" filtra a lista.
- **Apagar do histórico:** cola-se um trecho (uma senha, um dado colado por engano) e ele some de todas as versões da nota, ou de todas as notas do cofre. Pede 3 caracteres ou mais e avisa se a nota ainda tem o trecho (senão a próxima versão o traz de volta). Não dá para desfazer.

**Nas configurações** (Recuperação de arquivos), os dois campos que diziam "Sem efeito" agora valem:
- **Guardar todas as versões por N dias** (`keepDays`, padrão 1): nesse período nenhuma versão sai.
- **Intervalo das versões antigas** (`intervalMinutes`, padrão 5): depois disso fica 1 versão a cada N minutos até 7 dias, 1 por dia até 30 dias e 1 por semana para sempre. Com 0, todas ficam até 7 dias.
- A mudança vale 1 minuto depois do último ajuste (arrastar de 30 para 1 dia não apaga nada no caminho).
- **Espaço:** "N versões de M notas · X em disco" e o botão **Compactar agora**. A compactação também roda sozinha: depois de cada poda que tirou algo, ou quando o arquivo passa de 1 MB e do dobro do tamanho da última compactação.

**Por baixo:** `PoliticaRetencao` ganhou `intervalo` e `intervalo_ate` (o padrão do crate continua o da H1); `Config::politica()` traduz as configurações; a linha de fundo ganhou `nomear`, `estatisticas`, `compactar`, `apagar_texto_em_todas`, `podar_agora` e `ajustar_politica` (nada disso roda no quadro da tela). Testes de GUI sem janela para a barra, o coalescer, os nomes, a poda que poupa a nomeada, o apagar e a página de configurações. Prints em `C:/dev/_wt/prints-rustidian/h2/`.

**Também:** o teste `salvar_uma_copia_e_adicionar_propriedade...` falhava 6 em 20 mesmo sem carga. A causa não era o Enter: o salvamento automático da nota (600 ms depois) apaga a mensagem de erro do diálogo. O teste agora espera o salvamento; 20/20 e 5/5 com carga.

**Ficou para depois:**
- Teto de espaço por nota e por cofre (o briefing pede "configurável"): a compactação e a retenção seguram o crescimento, mas não há teto.
- O aviso "a nota ainda tem este trecho" olha o arquivo salvo, não o que está só no editor sem salvar.
- O salvamento automático apagar a mensagem de erro de um diálogo aberto parece um defeito do app (`after_note_saved` zera `error`); não foi mexido.
- `.canvas` continua fora do histórico.
- O histórico segue local, sem sync e sem cifra (fica para depois da H2, como já dizia o plano).
- Detalhes de acabamento: "1 dias" no singular e "5.0 min" com ponto (o campo numérico do egui usa sufixo e ponto fixos); os botões alternáveis "Só com nome" e "Em todas as notas do cofre" desligados parecem texto comum.

## H3/H4: plano (20/11/2026)

Ramo `hist-h3h4`, partindo da H2 (main). Duas fatias, um commit de plano e um por fatia (ou mais, pequenos).

### H3 Desfazer que fica

**Ideia.** O Ctrl+Z do egui (`TextEditState::undoer`) continua sendo quem desfaz na tela. A H3 mantém, ao lado, uma **árvore de desfazer** por nota no cofre de versões. Ela é alimentada pelo mesmo `editou` que já alimenta o histórico: a linha de fundo junta as edições e, depois de ~0,8 s sem digitar (ou 3 s de digitação contínua, ao salvar, ao trocar de nota e ao fechar), registra um **passo**. Se o texto novo é igual ao de um passo que já existe (o usuário apertou Ctrl+Z ou Ctrl+Y), a árvore só anda o ponteiro `atual`; se é um texto novo, nasce um filho do `atual`. Desfazer e digitar outra coisa vira um galho novo e o caminho antigo continua na árvore.

**Ao reabrir** (nota ou app), a linha de fundo confere o hash blake3 do texto do arquivo com o do passo `atual` (ou, se não bate, com qualquer passo da árvore) e devolve ao app os textos dos até 100 passos anteriores no caminho; o app semeia o `Undoer` do egui com eles (`add_undo`). Assim o Ctrl+Z comum, sem código novo de teclado, desfaz edições da sessão anterior. Se nenhum passo tem o texto do arquivo (o arquivo mudou fora do app ou pelo sync), a árvore **não se aplica**: ela é arquivada como "árvore antiga" (só histórico, até 2 por nota) e começa uma nova. Nunca se aplica passo sobre texto diferente.

**Tabelas novas no `redb`** (criadas na abertura, sem tocar nas existentes; `FORMATO` continua 1, então o cofre real abre igual e as versões não são mexidas):

| Tabela | Chave | Valor |
|---|---|---|
| `arvores` | (número da nota, geração) | cabeçalho: `atual`, próximo id, nº de passos, bytes, criada em, arquivada em (0 = ativa) |
| `nos` | (número da nota, geração, passo) | registro: pai, quando, tamanho, blake3 de 128 bits, trecho curto, "cheia" ou diferença, zstd |

O número da nota é o mesmo das versões (`notas`), então renomear a nota leva a árvore junto. Cada passo é uma **diferença zstd tendo o texto do pai como base** (um passo de ~50 caracteres custa ~100 bytes com o cabeçalho); a cada 32 passos de profundidade há uma cópia cheia, para reconstruir sem andar mais de 31 diferenças. A árvore é uma floresta: quando o limite descarta os passos mais antigos, os filhos deles viram raízes (regravados como cópia cheia).

**Limite por nota** (`LimitesDesfazer`): padrão 5.000 passos ou 2 MiB, o que vier primeiro; ao passar, descarta os mais antigos até 90% do limite. Nunca descarta o passo `atual`. O app passa o limite ao crate (`ajustar_limites_desfazer`).

**Galhos.** Um galho é uma folha da árvore (mais o passo `atual`, se não for folha). O diálogo "Galhos do desfazer" lista cada um com a hora, o trecho que aquele passo mudou ("+ texto novo" ou "apagou: ...") e quantos passos tem da raiz; marca o `atual` e, abaixo dele, os que dá para "refazer". Clicar em "Ir para este galho" põe o texto daquele passo na nota (como uma edição normal, que o Ctrl+Z desfaz) e refaz a semente do Ctrl+Z com o caminho dele.

**Cifra.** Igual à H1: o histórico local continua sem cifra e fora do sync (`.rustidian/` é ignorado). A árvore mora **só** nesse arquivo; nada dela vai para outro lugar.

**"Apagar do histórico" vale para a árvore.** `apagar_texto_do_historico` e `apagar_texto_de_todas` também reescrevem todos os passos (ativos e arquivados) da nota: o texto de cada passo perde o trecho, o hash, o resumo e a diferença são refeitos, e o arquivo é compactado. Teste: o trecho some dos bytes brutos do arquivo e dos textos dos passos.

### H4 Escritor (achar o que apaguei e histórico do trecho)

Tudo puro no crate (`escritor.rs`), sobre os textos das versões do H1 (leitura em lote, uma passada da mais nova para a mais antiga: `Historico::textos_da_nota`). Nada é guardado a mais: se o trecho foi tirado do histórico, ele não aparece (o teste cobre).

- **Achar o que apaguei.** Cada versão vira unidades (linha e frase; marcadores de lista, título e caixa de seleção saem da comparação). Uma unidade que existiu em alguma versão e **não está** no texto atual (comparação sem diferença de espaços) é um trecho apagado. Vale a mais longa (um trecho contido em outro apagado maior sai da lista). Cada item tem o texto, de quando a quando existiu e quando sumiu; a lista vem do mais recente para o mais antigo. Busca por palavra: sem diferença de maiúsculas e de acentos, todas as palavras digitadas precisam aparecer. Botões "Reinserir no cursor" (pelo mesmo caminho de edição do menu da nota, então o Ctrl+Z desfaz) e "Copiar".
- **Histórico deste trecho.** Com uma seleção, o crate leva a faixa selecionada de volta pelas versões usando a diferença por palavra entre cada par de versões vizinhas (a faixa de depois vira a faixa de antes; trocas e inserções dentro da faixa a ampliam até o trecho trocado). A lista traz só as versões em que o texto daquela faixa mudou (ou nasceu), da mais nova para a mais antiga, com "Copiar" e "Trocar a seleção por esta" (só vale se o texto da seleção ainda é o mesmo).
- **Comandos** (bloco "Histórico H3/H4" em `src/commands.rs`): "Achar o que apaguei", "Histórico deste trecho", "Galhos do desfazer". Aparecem na paleta; os dois primeiros e o terceiro também no menu de contexto da aba da nota, se houver menu (senão só paleta).

### Testes

Crate: ida e volta da árvore, galhos depois de desfazer+digitar, reabrir o arquivo, mudança de fora (arquivar), limite descartando os antigos, árvore sobrevive a `podar`/`compactar`/`renomear_nota`, migração (banco só com as tabelas do formato atual abre, ganha as novas e as versões continuam lendo), apagar do histórico (versões, passos e arquivo bruto), proptest da árvore (cada passo reconstrói com o hash gravado), achar apagados e histórico do trecho com textos conhecidos. GUI (`gui_tests/historico_h3h4.rs`): Ctrl+Z depois de reabrir o app, galho depois de desfazer+digitar e o diálogo, achar o que apaguei com busca e reinserir, histórico do trecho, apagar do histórico fazendo o trecho sumir do "achar". Print: `print_do_achar_apagado`.

### H3/H4: o que ficou pronto (20/11/2026)

**Onde está cada coisa.** Crate `nota-historico`: `desfazer.rs` (árvore, galhos, limites, arquivar, apagar), `escritor.rs` (achar apagados e histórico do trecho), e em `historico.rs` o `reconstruir_arquivo`. App: `historico_fundo.rs` (a linha de fundo registra a árvore e atende os pedidos), `historico_ui.rs` (semeia o Ctrl+Z com o que a linha de fundo manda), `editor_ui.rs` (`seed_undoer`, seleção, troca de caracteres, itens do menu da nota), `escritor_ui.rs` (o diálogo único das três telas), `recovery.rs` e `settings_ui.rs` (passos e KB do desfazer por nota), `commands.rs` (bloco "Histórico H3/H4").

**Como a pessoa usa.**
- Fechar e abrir o app (ou a nota): o Ctrl+Z continua de onde parou, até 99 passos para trás. Só semeia se o desfazer da nota ainda está vazio: o que a pessoa já fez nesta sessão não é trocado.
- Paleta (Ctrl+P) e menu de contexto da nota: "Galhos do desfazer", "Achar o que apaguei" e "Histórico deste trecho" (este precisa de uma seleção).
- Ajustes, "File recovery": "Passos do desfazer por nota" (padrão 5000) e "Espaço do desfazer por nota" (padrão 2048 KB).

**Decisões que mudaram em relação ao plano.**
- "Ir para este galho" entra como uma **edição normal** (troca só o miolo que difere, para o cursor e o resto da nota ficarem). O Ctrl+Z volta para onde a pessoa estava, e a árvore anda o ponteiro. Os passos anteriores do galho já existem na árvore, mas o Ctrl+Z do egui só os conhece na próxima abertura da nota (a semente).
- A árvore só registra depois da pausa na digitação (0,8 s; teto de 3 s) e ao salvar, trocar de nota, fechar e encerrar o app. Se o histórico é limpo ("Apagar todo o histórico"), as árvores vão junto e a próxima edição de cada nota abre outra.
- Arquivo de fora: se o texto do arquivo não é o de nenhum passo, a árvore antiga é arquivada (máx. 2 por nota, só aparece como "Desfazer de antes de uma mudança de fora do aplicativo") e começa uma nova. Nada é aplicado sobre texto diferente.
- **Apagar do histórico** vale para a árvore e para "Achar o que apaguei". Como o `redb` deixa bytes velhos nas páginas depois de reescrever, `reconstruir_arquivo` copia tudo para um arquivo novo e o troca pelo antigo (o teste confere os bytes brutos). Custa uma cópia do histórico inteiro, só ao apagar.

**Limites conhecidos.** Os pontos de desfazer semeados não têm posição de cursor exata (usa o início da diferença). O diálogo mostra até 60 trechos apagados de uma vez (a busca estreita). O histórico continua sem cifra e fora do sync, como na H1.
