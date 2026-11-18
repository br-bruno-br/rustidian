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
