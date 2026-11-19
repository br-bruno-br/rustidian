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
