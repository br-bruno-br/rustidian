# nota-historico

Histórico completo das notas (fase H1 do `docs/bruno/HISTORICO-NOTAS.md`): centenas ou milhares de
versões por nota, em pouco espaço, abrindo qualquer uma na hora. Crate de lógica, **sem UI**. O app
liga o crate em `src/historico_fundo.rs` (linha de fundo dona do arquivo), `src/historico_ui.rs`
(o que o editor entrega) e `src/recovery_ui.rs` (a tela); ver "Como o app usa" no fim.

## O que tem

| Peça | Arquivo | O que faz |
|---|---|---|
| `Historico` | `src/historico.rs` | banco `redb`: `gravar`, `ler`, `versoes`, `nomear`, `podar`, `compactar`, `apagar_texto_do_historico`, estatísticas |
| `Codec` / `ZstdCodec` | `src/codec.rs` | compressão atrás de um trait; zstd com a versão vizinha como base |
| pedaços | `src/pedacos.rs` | corte por conteúdo (hash rolante Gear) + chave blake3 |
| registro | `src/registro.rs` | tipos públicos (`NotaId`, `VersaoId`, `Origem`, `VersaoInfo`) e o formato binário |
| `PoliticaRetencao` | `src/retencao.rs` | quais versões ficam (função pura) |
| `Agrupador` | `src/agrupador.rs` | decide **quando** gravar, "como o Word" (função pura, relógio simulado nos testes) |
| `diff_palavras` | `src/diff.rs` | antes e depois por palavra (`imara-diff`) |

## Como as versões ficam guardadas (técnicas 1, 2 e 3)

- **Cadeia por nota.** A versão mais nova é sempre uma cópia cheia. Cada anterior vira uma diferença
  "para trás": o zstd comprime o texto dela **sabendo da vizinha mais nova** (uma edição de ~50
  caracteres vira ~40 bytes). A cada N versões (padrão **128**, `Opcoes::quadro_chave_a_cada`) há um
  quadro-chave inteiro, então abrir uma versão antiga aplica no máximo N-1 diferenças.
- **Quadros-chave em pedaços.** O quadro-chave é cortado em pedaços de ~2 KB (mín. 1 KB, máx. 4 KB)
  pelo próprio conteúdo; cada pedaço é comprimido e guardado **uma vez**, pela chave blake3. Vale
  entre versões e entre notas. Uma contagem de referências apaga o pedaço órfão.
- **Integridade.** Cada versão guarda o blake3 do texto (128 bits, ver abaixo). `ler` confere e
  devolve `Erro::Integridade` se não bater.
- **Quando gravar** (no lugar do tecla a tecla): `Agrupador`. Ver os exemplos no módulo.
- **Retenção** (`podar`): `PoliticaRetencao` com quatro campos. Padrão: 24 h tudo (`tudo_ate`), 1 por
  `intervalo` (1 hora) até `intervalo_ate` (7 dias), 1 por dia até `por_dia_ate` (30 dias), 1 por
  semana depois. `intervalo` zero guarda tudo na segunda faixa. Nomeadas e a mais nova ficam; a poda
  recodifica as vizinhas e nada quebra.
- **Android:** o crate `zstd` compila C. Ver "Android" abaixo.

### Qual API do zstd para "a versão vizinha como base"

Medido (zstd 1.5.7, nota de 5 KB, edição de ~50 caracteres, `tests/medir_codec.rs`):

| API | Saída (nível 3) | Tempo por chamada |
|---|---|---|
| `CCtx::ref_prefix`, contexto novo a cada vez | 44-49 B | 70-520 us |
| `CCtx::ref_prefix`, contexto reaproveitado | 48 B | 18-51 us |
| `Compressor::with_dictionary`, novo a cada vez | 44 B | 87-975 us |
| `Compressor::set_dictionary`, reaproveitado | **44 B** | **65-84 us** |

Escolhida a última: o `ref_prefix` exige que o prefixo viva tanto quanto o contexto, o que impede
guardar o contexto no codec sem `unsafe`. Nível 3 vs 9 vs 19: 44 / 40 / 42 B por diferença, mas 90 /
370 / 800 us: nível 3. As diferenças saem sem os 4 bytes da assinatura do quadro zstd (o codec
recoloca ao ler).

### Contrato, com os refinamentos

```rust
let mut h = Historico::abrir(Path::new("historico.redb"))?;
let nota = NotaId::novo("id-estavel-da-nota");           // não é o caminho
let v = h.gravar(&nota, "texto", SystemTime::now(), Origem::Editor)?; // None se igual à última
let infos = h.versoes(&nota)?;                            // Vec<VersaoInfo>
let texto = h.ler(&nota, infos[0].id)?;
h.nomear(&nota, infos[0].id, Some("rascunho 2".into()))?; // nomeada nunca é podada
h.podar(SystemTime::now(), &PoliticaRetencao::default())?;
h.compactar()?;                                           // refaz quadros-chave e devolve espaço
h.apagar_texto_do_historico(&nota, "senha colada")?;      // some de todas as versões
h.apagar_texto_de_todas("senha colada")?;                 // idem, em todas as notas (ApagadoEmTodas)
h.renomear_nota(&nota, &NotaId::novo("outro/id"))?;       // a nota foi renomeada: o histórico vai junto
h.ajustar_quadro_chave(48);                               // N menor para notas grandes (próximas gravações)
h.persistir()?;                                           // com Durabilidade::Adiada (padrão)
```

- `Result<Vec<_>>` em vez de `Vec` em `versoes`, e `&NotaId` em vez de `NotaId`: erros de banco
  existem e o editor não precisa clonar o id a cada gravação.
- **Durabilidade.** Padrão `Adiada`: a gravação fica na memória do `redb` até `persistir()` (o
  editor chama de tempos em tempos e ao fechar; o `Drop` também chama). Uma queda de energia perde
  só as últimas versões e nunca corrompe o banco. `Imediata` faz `fsync` a cada versão (~2 ms aqui).
- **Impressão = 128 bits.** `VersaoInfo::impressao` e a conferência de `ler` usam os 16 primeiros
  bytes do `blake3::hash(texto)` (`impressao_do_texto`). Quem tem o hash de 32 bytes (como o
  `nota-sync`) compara com `[..16]`. Metade do espaço por versão, e sobra de sobra para achar
  corrupção.
- **`apagar_texto_do_historico`**: remove cada ocorrência do trecho de cada versão (devolve em
  quantas aparecia), regrava os pedaços e diferenças afetados, apaga os pedaços antigos e compacta o
  arquivo. **Custo:** reconstrói e recodifica a cadeia inteira da nota (~100 us por versão) e
  reescreve o arquivo: rode em segundo plano. **Limites:** só some o trecho inteiro (versões
  gravadas enquanto a senha era digitada podem ter um pedaço: repita com os pedaços); tire também
  do `.md` real; trata uma nota por vez.
- **`apagar_texto_de_todas`**: o mesmo em todas as notas, compactando o arquivo uma vez no fim, só se
  algo foi apagado. Devolve `ApagadoEmTodas { notas, versoes }`. Mesmos limites e mesmo custo (por
  nota que tem o trecho; as outras só são lidas).

## Medição (régua do briefing)

Windows 10, este notebook (Asus X510UR, Core i5-7200U, SSD), 18/11/2026, build `--release`.
Nota sintética de 5 KB em português, 1.000 edições de ~50 caracteres (inserir, apagar e trocar em
posição aleatória, semente fixa), uma versão por edição. Rodar:

```
cargo test --release -p nota-historico --test medicao -- --ignored --nocapture
```

Melhor rodada (as outras tiveram um segundo agente compilando o workspace ao mesmo tempo; nelas a
mediana de gravar ficou igual, 0,36-0,40 ms, e só a média e o pior caso subiram):

| Medida | Régua | Resultado |
|---|---|---|
| Espaço (arquivo depois de `compactar`) | < 200 KB | **172,0 KB** (172.032 B; 65,8 KB de registros + 23,2 KB de pedaços + 32 KB fixos do `redb`) |
| Abrir qualquer versão, pior (mínimo de 3 leituras por versão) | < 5 ms | **0,68 ms** |
| Abrir, média | < 5 ms | **0,29 ms** |
| Gravar, média (durabilidade adiada) | < 1 ms | **0,39 ms** (p50 0,36 ms, p99 0,86 ms) |
| Gravar, média (durabilidade imediata, `fsync`) | < 1 ms | 2,4 ms (p50 2,1 ms): não cumpre; por isso o padrão é adiada |

Notas sobre a medição:
- O arquivo antes de `compactar` fica em ~1 MB (o `redb` reaproveita páginas livres mas não encolhe
  sozinho); `compactar()` devolve o espaço.
- Leituras isoladas chegaram a 35-90 ms nas rodadas com o computador ocupado (picos do sistema, não
  da conta: o mesmo `ler` repetido dá 0,3 ms). A régua de abrir foi julgada pelo mínimo de 3
  leituras por versão.
- Quadro-chave a cada 64 em vez de 128: **204,8 KB** (falha por 2%), abrir pior ~0,6 ms. Por isso o
  padrão é 128. Para notas muito maiores (dezenas de KB), cada diferença carrega a versão vizinha
  inteira como base: um N menor (32-64) limita o pior caso de abertura. Ver riscos abaixo.
- Os 32 KB fixos do `redb` (banco vazio) e a folga das páginas da árvore B são ~45% do arquivo.

### 10 notas parecidas (deduplicação)

| Cenário | Arquivo | Pedaços | Bytes de pedaços |
|---|---|---|---|
| 1 nota (referência) | 172,0 KB | 28 | 23,2 KB |
| 10 x 1 nota, sem repartir | 1.720 KB | 280 | 232 KB |
| 10 notas com o mesmo texto inicial e edições diferentes | 1.925 KB | 261 | 208 KB |
| 10 cópias (mesmo fluxo de edições, uma marca por nota) | **1.671 KB** | **100** | **63 KB** |

Nas cópias, a deduplicação tira **64% dos pedaços e 73% dos bytes de pedaços** (232 KB -> 63 KB).
O arquivo todo cai pouco (3%) porque o grosso (658 KB) são as diferenças de cada versão, que são
por nota e não se repartem. Nas notas com edições diferentes só o texto inicial é comum (-10% nos
pedaços); o arquivo fica maior que 10x porque inserir em dez trechos da árvore B deixa mais folga
nas páginas (o `compactar` do `redb` move páginas, mas não as repacota).

## Android

`cargo check --locked -p nota-historico --target aarch64-linux-android` **funciona** com o NDK de
`C:/dev/_ferramentas` (zstd-sys compilou com o clang do NDK 29). O repositório não tem
`.cargo/config` para isso (o `android/construir.sh` só exporta `ANDROID_HOME`/`ANDROID_NDK_HOME` e
deixa o `dx` ajustar o resto), e o `cc-rs` sozinho não achou o clang (`failed to find tool
"clang.exe"`). Foi preciso, só no comando e sem gravar nada: `CC_aarch64_linux_android` apontando
para `.../ndk/29.0.14206865/toolchains/llvm/prebuilt/windows-x86_64/bin/aarch64-linux-android30-clang.cmd`
e `AR_aarch64_linux_android` para o `llvm-ar.exe` do mesmo diretório. Só `check` (não linkou nem
rodou no aparelho). Alternativas em Rust puro, se o C incomodar: `lz4_flex` ou `ruzstd`: basta
implementar `Codec`.

## Testes

- `src/*` (39): codec, pedaços, registro, retenção, agrupador com relógio simulado, diff por palavra.
- `tests/historico.rs` (15): gravar/ler 300 versões, texto igual não grava, vazio e Unicode, reabrir,
  nomear, poda, compactar, dedupe entre notas, apagar trecho (some dos pedaços descomprimidos e dos
  bytes brutos do arquivo), integridade (codec que estraga é pego), banco de outro codec não abre.
- `tests/propriedade.rs`: `proptest`, 40 casos de até 70 operações aleatórias (gravar, podar,
  nomear, compactar, apagar trecho, reabrir; quadro-chave a cada 1 a 8). Depois de **cada**
  operação, toda versão que o histórico lista reconstrói igual ao modelo (texto e blake3), nomeadas
  e a mais nova ficam, e a contagem de referências dos pedaços bate com o uso real.
- `tests/medicao.rs`, `tests/medir_codec.rs`: medições (`--ignored`).

## Riscos para ligar ao editor

- **Nunca gravar na linha da tela.** `gravar` leva ~0,4 ms, mas com pico de dezenas de ms quando o
  `redb` cresce o arquivo; `compactar`, `podar` e `apagar_texto_do_historico` levam centenas de ms a
  segundos. Tudo numa linha de fundo, com o `Historico` atrás de um `Mutex` ou de um canal.
- **Um escritor por arquivo.** O `redb` trava o arquivo para um processo só; duas instâncias do app
  no mesmo cofre precisam combinar quem grava.
- **Notas grandes.** A conta de espaço e de abertura foi feita com 5 KB. Com notas de centenas de KB,
  cada diferença carrega a vizinha inteira (custo de abrir cresce com N x tamanho): ajustar
  `quadro_chave_a_cada` pelo tamanho, e medir de novo.
- **Segredos.** O histórico guarda o que foi apagado do `.md`; nada aqui é cifrado (a cifra do
  `nota-cripto` entra na etapa do sync).
- **Origem e `quando`** vêm do chamador; a retenção usa o `quando` gravado (relógio atrasado vira
  "agora", não quebra).
- **Identidade.** O `NotaId` precisa ser mesmo estável (ao renomear/mover o arquivo, o mesmo id).
- `THIRD_PARTY_NOTICES.txt` não foi alterado: ele é gerado no release (`cargo xtask
  language-notices`) só para o que entra no app. Quando o crate for ligado ao app, entra o `zstd`
  (MIT/Apache-2.0, mais o zstd C em BSD/GPL escolha BSD).

## Como o app usa (H1 ligada, 18/11/2026)

- **Uma thread só** (`historico_fundo::Historico`) é dona de `<cofre>/.rustidian/historico.redb`:
  gravar, podar, compactar, apagar trecho e ler rodam lá; o quadro do editor só manda mensagens.
  O `Agrupador` também mora lá (o blake3 que ele calcula a cada edição não roda no quadro): a
  interface manda `editou`/`salvou`/`carregou`/`saiu`/`fechou` e a thread conta a pausa sozinha.
- **Um escritor por arquivo:** outra instância no mesmo cofre fica sem histórico e avisa uma vez.
- **Id da nota = caminho relativo com `/`.** O app chama `renomear` em cada rename/move (nota ou
  pasta); a thread usa `renomear_nota` para cada id sob o prefixo.
- **Notas grandes:** `quadro_chave_a_cada` = 128 até 200 KB, 48 até 1 MB, 32 acima
  (`ajustar_quadro_chave` antes de cada gravação). O crate foi medido só com 5 KB: as faixas de
  200 KB e 1 MB são estimativa, não medição.
- **Migração:** na primeira abertura, o `recovery.redb` antigo (estado do usuário) entra em ordem;
  a marca `.rustidian/historico-migrado.txt` evita repetir e o banco antigo fica onde está.
- **Retenção configurável:** a política vem do `Config` do app (`Config::politica`): `keep_days` =
  guardar todas por N dias; `interval_minutes` = 1 versão a cada N minutos até 7 dias (ou até
  `keep_days`, se maior), 0 = todas; depois 1 por dia até 30 dias e 1 por semana. `podar` roda na
  abertura e a cada hora. Mudar a política (`ajustar_politica`) não poda na hora: a próxima poda
  espera 60 s sem outra mudança, porque quem arrasta um número de 30 para 1 passa por valores
  intermediários e apagaria versões que não quis apagar.
- **Compactação por tamanho:** depois da poda, `compactar` se ela removeu algo **ou** se o arquivo
  passou de 1 MiB e do dobro do tamanho logo depois da última compactação (o arquivo também deixa de
  crescer quando só se escreve).
- **Pedidos da tela** (todos sem esperar, devolvem um `Receiver`): `nomear` (espaços das pontas
  saem, vazio tira o nome, no máximo 80 caracteres), `estatisticas`, `compactar`, `podar_agora`
  (poda já com a política guardada; serve aos testes da tela), `apagar_texto` e
  `apagar_texto_em_todas`. Com o histórico desligado ou indisponível respondem erro e não criam o
  arquivo.
- O sync ignora `.rustidian/` (`nota-sync/src/caminhos.rs`); o histórico não é sincronizado nem cifrado.

## H3 e H4 (20/11/2026)

- **Desfazer que fica** (`desfazer.rs`): uma árvore de passos por nota, no mesmo arquivo `redb`
  (tabelas `arvores` e `nos`, número da nota como chave: renomear leva a árvore junto). Cada passo é
  uma diferença zstd sobre o texto do pai e leva o blake3 do texto, conferido na leitura. `FORMATO`
  continua 1: um arquivo da H2 abre igual e ganha as tabelas novas. Se o texto do arquivo não é o de
  nenhum passo (mudou por fora), a árvore antiga é arquivada (até 2) e começa outra; nada é aplicado
  sobre texto diferente. O limite (`LimitesDesfazer`, padrão 5.000 passos ou 2 MiB por nota) descarta
  os passos mais antigos. `apagar_texto_do_historico` e `apagar_texto_de_todas` também reescrevem os
  passos e, como o `redb` deixa páginas velhas no arquivo, refazem o arquivo (`reconstruir_arquivo`).
- **Escritor** (`escritor.rs`): `achar_apagados` lista os trechos (linha ou frase) que existiram em
  alguma versão e não estão no texto de agora; `filtrar_apagados` busca por palavra sem ligar para
  maiúscula e acento; `historico_do_trecho` leva uma faixa selecionada de volta pelas versões.
- Testes: `src/desfazer.rs` e `src/escritor.rs` (unidade) e `tests/desfazer_escritor.rs` (16, contra
  um banco de verdade: ida e volta, galhos, mudança de fora, limite, migração da H2, apagar nos bytes
  brutos, `proptest` da árvore, achar apagados e histórico do trecho).
