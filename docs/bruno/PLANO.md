# Plano do fork (br-bruno-br/rustidian)

Objetivo do dono (15/11/2026): largar o Obsidian e usar um editor de notas em Rust no **Windows** (este PC), no **Android** e no **Linux**. As notas ficam sincronizadas entre os três, com o **re-servidor** (notebook HP com Ubuntu, sempre ligado, na Tailscale) como ponto de encontro. Se o re-servidor cair, ninguém perde acesso: cada aparelho guarda a cópia completa. Além disso, um **backup criptografado no Google Drive**.

Este repositório é **público**. Aqui entra só código e documentação. Nunca nota, chave, token ou senha.

## Princípios

1. **Nota nunca se perde.** Na dúvida, a sincronização guarda as duas versões. Nada apaga de vez: o que some vai para a lixeira do cofre.
2. **O servidor não lê as notas.** Ele guarda só conteúdo cifrado (criptografia de ponta a ponta). O Google só recebe o backup cifrado.
3. **Funciona sem rede.** Cada aparelho edita a sua cópia e acerta quando a rede volta.
4. **Arquivos comuns.** O cofre continua sendo uma pasta de Markdown. Isso permite voltar ao Obsidian a qualquer hora.
5. **Fatias pequenas, cada uma testável sozinha.** Um commit por peça, e o gate passa antes de juntar.

## Arquitetura

```
 Windows (app egui) ──┐                      ┌── backup diário (rustic, cifrado) ──► Google Drive
 Android (app)  ──────┼── HTTPS na Tailscale ─► re-servidor: nota-servidor (só cifrado)
 Linux (app egui) ────┘      (token por aparelho)
        └── todos usam o mesmo miolo: crates/nota-cripto + crates/nota-sync
```

### Crates novas
- `crates/nota-cripto`: chave do cofre e cifra de arquivo e nome. Sem rede e sem disco além do que receber.
- `crates/nota-sync`: protocolo, estado local, varredura do cofre, cliente HTTP, resolução de conflito. Testável com servidor em memória.
- `crates/nota-servidor`: binário axum que roda no re-servidor. Guarda blobs cifrados e o índice em SQLite.
- O app (raiz) continua sendo o pacote `rustidian` e ganha a integração do cliente de sync.

### Criptografia (`nota-cripto`)
- **Chave do cofre K:** 32 bytes aleatórios, criada uma vez.
- **Arquivo `cofre.chave`:** guarda K embrulhada por uma chave derivada da frase-senha. A derivação é Argon2id (m = 64 MiB, t = 3, p = 1, sal de 16 bytes), e o embrulho é XChaCha20-Poly1305. O arquivo fica no estado do app (fora do cofre) e no servidor, porque cifrado ele não serve a quem não tem a frase. Cada aparelho digita a frase uma vez e guarda K no cofre do sistema (Windows: DPAPI/Credential Manager; Android: Keystore). A frase-senha fica no gerenciador de senhas do dono, e só lá.
- **Subchaves por HKDF-SHA256:** `k_conteudo` e `k_nome`.
- **Id de arquivo:** HMAC-SHA256(k_nome, caminho normalizado: `/`, NFC, sem `./`) em hex. Determinístico, então os aparelhos concordam sem conversar.
- **Blob:** versão(1) ‖ nonce(24) ‖ XChaCha20-Poly1305(k_conteudo, AAD = id ‖ versão do formato, texto = caminho_len(u32) ‖ caminho ‖ conteúdo). O servidor vê só o id e o tamanho.
- **Teste obrigatório:** byte trocado, AAD trocado, frase errada e K errada falham, e o round-trip funciona com 0 bytes, 1 byte e 10 MB.

#### Desvios da R3
- **`id_de` devolve `Result<IdArquivo, Erro>`**, não `IdArquivo`: o caminho passa por `normalizar`, que recusa `..`, absoluto e vazio, e não dá para esconder isso num retorno sem erro.
- **Formato do `cofre.chave` v1** (o PLANO só dizia "versionado"): `versão(1) ‖ m_kib(u32 LE) ‖ t(u32 LE) ‖ p(u32 LE) ‖ sal(16) ‖ nonce(24) ‖ cifra de K + tag(16)`, 101 bytes. O cabeçalho (29 bytes) entra como AAD, então trocar um parâmetro invalida o arquivo. Ao ler, a crate recusa parâmetros acima de 1 GiB de memória, t > 64 ou p > 64 (`Adulterado`), para um arquivo adulterado não forçar alocação enorme.
- **Frase em NFC** antes do Argon2id, para teclados e sistemas que normalizam Unicode de jeitos diferentes abrirem o mesmo cofre.
- **API a mais:** `ChaveCofre::de_bytes(&[u8])` e `ChaveCofre::bytes() -> Zeroizing<[u8; 32]>`, para guardar K no cofre do sistema (Credential Manager, Keystore) sem passar pela frase. Tamanho diferente de 32 vira `Erro::Adulterado` (não criei variante nova). Também é público `TAMANHO_MAXIMO` (50 MiB).
- **Limite de 50 MiB vale para o conteúdo.** O blob fica até ~100 bytes maior (versão, nonce, tag, caminho): o servidor e o `nota-sync` devem aceitar essa folga ao aplicar o limite de blob. `decifrar` recusa blob acima de conteúdo máximo + 4 KiB de caminho + sobrecarga (`TamanhoExcedido`) antes de alocar.
- **Caminho:** máximo de 4096 bytes, sem NUL, sem letra de unidade (`C:`); barras repetidas e `.` somem. A crate **não** dobra maiúsculas/minúsculas: dois arquivos que só diferem na caixa têm ids diferentes (conflito no Windows é problema do `nota-sync`).
- **Frase errada e `cofre.chave` adulterado:** autenticação falhou com os parâmetros intactos é `FraseErrada`; tamanho errado, parâmetros fora do limite e sal/nonce trocados chegam como `FraseErrada` ou `Adulterado` (sem como distinguir de frase errada quando só a cifra mudou). Versão desconhecida é `FormatoDesconhecido`.
- **Vetor fixo conferido por implementação independente** (HKDF, HMAC e XChaCha20-Poly1305 refeitos em Python com HChaCha20 próprio): id e blob batem byte a byte. O vetor do `cofre.chave` (Argon2id) não foi conferido fora do Rust.

### Protocolo de sync (`nota-sync` ↔ `nota-servidor`)
- **Servidor:** tabela `itens(id PK, versao INTEGER, hash_blob TEXT, apagado INTEGER, seq INTEGER)`, blobs em `blobs/<sha256>` e um contador global `seq`. Só escuta no IP da Tailscale (100.x). Autenticação por `Authorization: Bearer <token do aparelho>`, com o servidor guardando só o SHA-256 dos tokens. Limite de tamanho por blob de 50 MB.
- **Rotas:** `GET /v1/mudancas?desde=<seq>`, `GET /v1/blob/<hash>`, `PUT /v1/blob` (corpo = blob; responde o hash) e `POST /v1/item/<id> {versao_base, hash_blob, apagado}`. A última responde 200 com a nova versão se `versao_base` for a atual, e **409** se não for.
- **Cliente:** banco local (redb) com uma linha por caminho: `id, versao_sinc, hash_texto_sinc (BLAKE3)`. Um ciclo:
  1. varre o cofre e acha as mudanças locais (`hash != hash_texto_sinc`);
  2. puxa as mudanças remotas desde o último `seq`;
  3. junta os dois lados:
     - mudou só do lado remoto: escreve o arquivo (escrita atômica; se o arquivo mudou no meio, vira conflito);
     - mudou só do lado local: envia;
     - **mudou nos dois lados:** a versão remota vira o arquivo principal, e a local é salva como `nome (conflito <aparelho> <AAAA-MM-DD HHmm>).md`. Nada se perde.
  - **Apagar:** vira lápide no servidor. No aparelho, o arquivo vai para a lixeira do cofre (`.trash/`), nunca para o `rm`.
- **Ignorados:** `.trash/`, `.rustidian/`, `.obsidian/workspace*`, arquivos temporários do editor.
- **Teste de propriedade obrigatório:** 3 aparelhos simulados com edições, criações, apagamentos e renomeações aleatórias, e quedas de rede injetadas no meio do envio. Ao fim, todos convergem e todo conteúdo que existiu aparece num arquivo ou na lixeira.

#### Desvios da R4
- **Cursor e caixa de entrada (buraco do desenho original).** Se o `seq` avançasse ao puxar e a mudança remota depois fosse adiada (arquivo mudou no meio, 409, queda), ela nunca mais voltaria. Agora o pull só grava o que o servidor disse numa tabela `remoto` (a caixa de entrada) e avança o cursor; a mudança está "pendente" enquanto `remoto.versao > base.versao`, e o motor decide sobre as pendentes a cada ciclo. Uma lápide de arquivo que o aparelho nunca viu também entra na base (`hash: None`) para o `versao_base` de uma criação posterior estar certo.
- **Corrida entre a varredura e a escrita.** O motor decide com o que viu na varredura; o executor confere de novo na hora do efeito. `Baixar`: se o arquivo não tem mais o hash da varredura, o que está lá é copiado para `nome (conflito ...)` antes de sobrescrever. `ApagarLocal`: só move para `.trash/` se o hash ainda for o da varredura; senão deixa o arquivo (a edição vence a lápide no ciclo seguinte). `Enviar`: relê o arquivo no envio e grava na base o hash do que foi de fato enviado. **Risco que sobra:** a janela entre conferir e o `rename` (microssegundos); no R5 o app deve segurar o salvar do editor durante o `ciclo`, ou salvar também de forma atômica.
- **409 traz o estado atual** (`versao_atual`, `hash_blob`, `apagado`). Serve para reconhecer a própria publicação cuja resposta se perdeu (mesmo `hash_blob`: conta como feita, sem duplicar) e para o cliente aprender a versão sem outra chamada. Por isso `Remoto::publicar` devolve `Result<Result<u64, Conflito409>, ErroRemoto>` (falha de transporte por fora, conflito de protocolo por dentro).
- **Motor puro com contexto.** O nome do aparelho e o carimbo `AAAA-MM-DD HHmm` viajam dentro de `EstadoBase`, para `reconciliar(local, base, remoto)` continuar sem relógio. O executor decifra o blob remoto **antes** de chamar o motor (o motor precisa do hash do texto para saber se "criado igual nos dois lados"). Ação a mais além das cinco do plano: `BaixarComo{id, nome}`, para um item remoto cujo caminho só difere na caixa de um arquivo daqui; o conteúdo vai para `nome (conflito ...)` e o id fica na base sem conteúdo.
- **Diário.** O plano inteiro vai para o diário (redb) antes do primeiro efeito; cada intenção termina na mesma transação que atualiza a base. `Enviar` tem duas fases (`Inicio`, `Enviado{hash_blob, hash_texto}`), gravadas antes de subir o blob e antes de publicar; repetir a publicação depois de uma queda cai no 409 com o mesmo hash e vira sucesso. Erro de disco numa intenção a encerra com registro (não trava o resto); erro de rede a mantém para o próximo ciclo.
- **Caminhos mais rígidos que `normalizar`.** Além do que a `nota-cripto` recusa, o sync recusa e registra (nunca escreve, nunca apaga) nomes que o Windows não aceita: `: * ? " < > | \`, controle, reservados (`CON`, `NUL`, `COM1`...), terminando em `.` ou espaço, e qualquer coisa em `.trash/`, `.rustidian/`, `.git/` ou `.obsidian/workspace*`. `.git/` é ignorada também (não estava na lista). Nome NFD em disco é achado por comparação em NFC antes de escrever (não cria um segundo arquivo ao lado).
- **Guarda contra exclusão em massa.** 20 ou mais lápides e metade ou mais dos arquivos sincronizados de uma vez (cofre vazio ou desmontado) dá `ErroSync::ExclusaoEmMassa` e nada é enviado, até `permitir_exclusao_em_massa(true)`.
- **Caixa (maiúsculas/minúsculas).** Entre dois caminhos que só diferem na caixa, o segundo (ordem de bytes) vira conflito com nome novo. Entre sistemas de arquivos diferentes isso não converge para o mesmo conjunto de nomes (um aparelho sensível à caixa terá os dois; um insensível terá uma cópia com nome de conflito), mas nada se perde. O teste de propriedade não usa nomes que só diferem na caixa; esse caso tem testes próprios no motor e no executor.
- **Servidor.** `PUT /v1/blob` aceita o cabeçalho `X-Hash-Sha256` e responde 400 se não bater (sem o cabeçalho, só devolve o hash calculado). `GET /v1/mudancas` é paginada (1000 itens; `{itens, seq, mais}`). Os tokens são relidos a cada pedido (token novo vale com o servidor no ar). `dados/config.json` pode trazer `escutar`. Blobs nunca são apagados (sem coleta de lixo; o R6 decide).
- **Cliente HTTP: `ureq` 3 sem TLS, trait `Remoto` síncrona.** O app roda o ciclo numa thread e o Android não precisa de runtime async; sem OpenSSL nem rustls. Só `http://`, sem redirecionamento, sem proxy do ambiente, tempos de 10 s (conectar) e 120 s (total). O tráfego já anda dentro da Tailscale (WireGuard) e o conteúdo é cifrado ponta a ponta; o token é o único segredo em claro no fio. Se um dia o servidor sair da Tailscale, falta TLS (reavaliar).
- **Lixeira do cofre.** `.trash/<pasta>/<nome> AAAA-MM-DD HHmmss.<ext>`. Hoje o app apaga pela lixeira do sistema (`trash`); o R5 tem de mandar o "apagar" do app para `.trash/` do cofre, senão uma exclusão feita no app não deixa cópia no cofre.

#### Desvios da R5
- **O salvar do editor e o ciclo se revezam num portão (`SaveGate`), e o ciclo o solta durante cada chamada ao servidor.** O `Sincronizador::ciclo` mistura disco e rede, e uma chamada pode levar até 120 s. Segurar o mutex o ciclo inteiro congelaria o Ctrl+S com o servidor fora do ar. O `Remoto` do app é embrulhado em `Gated<R>`: antes de cada chamada ele solta o portão e, na volta, toma de novo. Assim o portão fica preso só nos trechos de disco entre duas chamadas (varredura, conferir o hash, escrever, mover para `.trash/`), que é exatamente a janela entre "conferir e `rename`" dos desvios da R4. Custo: a varredura (só leitura, milissegundos) também roda sob o portão. O autosave (`PendingSave::write`), o Ctrl+S de nota nova (`Vault::save`) e o apagar para `.trash/` tomam o portão global (`sync::gate::global()`). Renomear e mover não tomam (uma nota que some no meio da varredura vira "sumiu antes de enviar" e nada se perde). Teste de concorrência: preso num efeito de disco, o salvar espera; preso numa chamada ao servidor, o salvar passa.
- **`cofre.chave` no servidor:** `GET /v1/chave` (404 se não há) e `PUT /v1/chave` (200 se gravou, 409 se já existe, 400 vazio, 413 acima de 4 KiB). A gravação é atômica (temporário + `hard_link`, que falha se o nome existe), então dois primeiros aparelhos ao mesmo tempo não se sobrepõem: o que perde baixa a chave do vencedor e abre com a mesma frase. `Remoto` ganhou `baixar_chave()` e `enviar_chave()` (`EnvioChave::{Gravada, JaExiste}`).
- **Frase mínima de 8 caracteres só para criar a chave** (primeiro aparelho). Para entrar num cofre que já existe basta a frase certa. A frase nunca é guardada nem registrada.
- **Guarda de segredos pela crate `keyring` 4.2 com a feature `v1`** (a mais enxuta que existe: no Windows entra só o `windows-native-keyring-store`, o Credential Manager). No Linux a `v1` traz o Secret Service via `zbus`; o crate recomenda `keyring-core` + a loja certa para quem quer controlar isso, e fica como alternativa se o peso do `zbus` incomodar. K e o token entram como bytes (`set_secret`), com a conta `<8 bytes do SHA-256 da pasta do cofre>:key|token`. Atrás da trait `SecretStore` (`KeyringStore`, `MemoryStore` para os testes). Nenhum teste grava no Credential Manager real. Em `<estado>/rustidian/sync/<hash>.json` fica só endereço, nome do aparelho, `connected` e `enabled`; o banco do `nota-sync` fica ao lado (`.redb`).
- **Disconnect** apaga K e o token do keyring, apaga o banco de estado do sync (é só cache do que já foi sincronizado; as notas não estão nele) e marca `connected = false`. Conectar de novo começa com estado limpo, o que evita um cursor de outro servidor. Notas, servidor e a lixeira ficam como estão.
- **Recarregar depois do ciclo:** a thread compara uma impressão digital barata do cofre (nomes, tamanhos e datas, sem ler conteúdo) antes e depois do ciclo. Se mudou, o app marca `refresh_required`, o mesmo caminho de uma edição externa: reindexa em segundo plano e recarrega só as abas **sem texto não salvo**. Ciclo ocioso não reindexa nada.
- **Estado na barra:** `Syncing…`, `Sync error`, `Offline`, `N conflicts`, `Synced HH:MM`, nessa ordem de prioridade; o clique abre a janela "Sync" com a lista de conflitos (clicáveis), os recusados da varredura, a mensagem do erro e, se o ciclo parou na guarda de exclusão em massa, o botão "Allow this deletion once" (vale para um ciclo; depois a guarda volta).
- **Apagar com sync ligado:** `Vault::trash_entry_in_vault` move para `.trash/` no formato do `mover_para_lixeira`, arquivo por arquivo (pasta inteira: cada arquivo vai com o mesmo carimbo e as pastas vazias saem). Com sync desligado, `trash_entry` (lixeira do sistema) continua igual.
- **`file://` no Windows:** `paths::file_uri` tira o prefixo `\?\` e produz `file:///C:/...` (UNC vira `file://servidor/...`). O teste só de Linux ganhou um par para Windows.
- **Disparos do ciclo:** a cada 60 s, 5 s depois do último salvar (debounce, no máximo 30 s depois do primeiro de uma sequência) e no botão. Apagar pelo app também dispara. Sem rede o estado vira `Offline`, nada trava e o próximo ciclo tenta de novo.

### Backup
- No re-servidor, o **rustic** (Rust, compatível com restic) guarda todo dia a pasta de dados do `nota-servidor` no Google Drive. Ele grava por meio do **rclone** (backend `rclone:gdrive:...`), decisão do dono em 15/11: o rclone faz o login no Google sem precisar de um cliente OAuth próprio. É a única peça fora do Rust, e só faz o transporte, porque a cifra é do rustic. Essa pasta já está cifrada, e o rustic cifra de novo.
- Retenção: 7 diários, 4 semanais e 12 mensais.
- `rustic check` toda semana e teste de restauração todo mês, num diretório descartável.
- Para restaurar: rustic restore, depois o `nota-servidor`, depois o cliente com a frase-senha.
- **Duas senhas** ficam no gerenciador de senhas do dono: a do rustic e a do cofre.

## Fatias

| # | Fatia | Pronto quando |
|---|---|---|
| R0 | Base: gate local no Windows e este plano | `cargo fmt --check`, `clippy -D warnings` e `cargo test` rodam (o que for Linux-only fica marcado) |
| R1 | Correções da revisão de segurança (15/11): imagem remota desligada por padrão (opção para ligar); aba do agente atrás da feature `agente` (desligada); `.rustidian/agent-context.md` fora do cofre; histórico de recuperação com padrão de 1 dia e chave para desligar | testes novos e nenhuma requisição de rede ao abrir uma nota com `![](http://…)` |
| R2 | Windows: seletor de arquivo com `rfd`, tema do sistema, fontes do sistema, instância única, terminal (ConPTY ou desligado) e `cfg` nos trechos Linux | `cargo test` no Windows verde, o app abre o demo-vault, edita e salva (captura conferida) |
| R3 | `nota-cripto` | testes do formato, adulteração, frase errada, vetores fixos |
| R4 | `nota-sync` + `nota-servidor` (em memória e em disco) | teste de propriedade com 3 aparelhos e quedas; servidor recusa sem token |
| R5 | Sync no app desktop: tela de configurar (endereço, token, frase), ícone de estado, ciclo em segundo plano | dois apps no mesmo PC com cofres diferentes convergem pelo servidor local |
| R6 | Implantar no re-servidor (systemd, só Tailscale) e backup rustic → Google Drive | serviço volta sozinho após reinício; backup e restauração testados |
| R8 | (R8a feita, R8b abaixo) Tudo em Rust: os scripts Python de empacotamento, benchmark e release viram um `xtask` (`cargo xtask ...`). O smoke de GUI (`tests/gui_smoke.py`, ~12 mil linhas, dirigido por X11) vira testes Rust com `egui_kittest`, que funcionam no Windows e no CI sem X11. O Python sai em fatias, só quando a cobertura equivalente existir | `git ls-files '*.py'` vazio; os mesmos fluxos cobertos por `cargo test` |
| R7 | Android: protótipo comparando egui (android-activity) e Dioxus para digitar acentos pelo Gboard, selecionar e rolar; depois o app com o mesmo miolo | APK instalado no celular do dono, sincronizando |
| R16 | Conferir a assinatura das atualizações antes de instalar (só local, sem rede): `cargo xtask release` gera `SHA256SUMS` + assinatura ed25519; `rustidian --install` e `rustidian --verificar` só aceitam o que bate com a chave pública embutida | `cargo test` com assinatura válida, artefato trocado, lista editada, outra chave e arquivo faltando |
| R14 | Quadros `.canvas` (JSON Canvas 1.0, o formato aberto do Obsidian): abrir numa aba e ver/editar cartões de texto (Markdown simples), de nota (clique abre a nota), de link e grupos, arestas com seta e rótulo; arrastar, redimensionar, criar (duplo clique), ligar, apagar, zoom e pan, desfazer; "Novo quadro" na paleta e no menu de pasta. Salva pelo mesmo caminho das notas (autosave e portão do sync). Código nosso (`src/canvas.rs` sem tela, `src/canvas_ui.rs` com a tela); só a ideia veio do RustSidian | `.canvas` escrito pela spec abre, edita e volta ao disco com todos os campos desconhecidos preservados (testes de ida e volta e de GUI sem janela); JSON inválido mostra aviso e nunca é sobrescrito |

R2 e R3 tocam arquivos diferentes e podem correr em paralelo.

## R8a: xtask e primeiros testes de GUI em Rust (feito)

**Linhas de Python: 14.781 → 13.343** (`git ls-files '*.py'`: 23 arquivos → 8).

### O que virou `cargo xtask` (crate `xtask/`, alias em `.cargo/config.toml`)
| Antes | Agora |
|---|---|
| `scripts/benchmark-vault.py`, `benchmark-editor.py`, `check-editor-timings.py` | `cargo xtask benchmark-vault`, `benchmark-editor`, `check-editor-timings` |
| `scripts/package-linux.py`, `package-deb.py`, `package-appimage.py` | `cargo xtask package-linux`, `package-deb`, `package-appimage` |
| `scripts/install-linux.py` (o `install.py` do tar.gz) | `rustidian --install` (`src/install_linux.rs`, só biblioteca padrão); o pacote não precisa mais de Python |
| `tests/linux_bundle.py`, `tests/debian_package.py` | `cargo xtask test-bundle`, `test-deb` (só Linux) |
| `tests/launcher_path.py` | teste unitário em `src/install_linux.rs` (roda o lançador no Unix) |
| `scripts/lint-packaging.py` | `cargo xtask lint-packaging` (roda no Windows; usa o bash do Git) |
| `scripts/language-notices.py` | `cargo xtask language-notices` |
| `scripts/preflight-release.py`, `publish-checked-release.py`, `reuse-actions-candidate.py` | `cargo xtask preflight-release`, `publish-release`, `reuse-candidate` |

Mudanças de comportamento de propósito: o lançador do `.deb` virou shell e o pacote deixou de depender de `python3`; o tar.gz sai reprodutível (data de `SOURCE_DATE_EPOCH`, dono `root`, modos explícitos); `START_HERE.txt` e o README mandam rodar `./rustidian --install`; a regra `python-syntax` do lint sumiu (o compilador confere). O que não deu para rodar no Windows (`package-deb`, `package-appimage`, `test-bundle`, `test-deb`, `preflight-release`, `publish-release`, `reuse-candidate`, `language-notices`) está compilado, com testes unitários das partes puras, mas só roda de verdade no Linux/Actions: a primeira rodada do CI é o teste real.

### Testes de GUI com `egui_kittest` (`src/app/gui_tests.rs`, `cargo test --bin rustidian`)
O app inteiro roda dentro do harness em memória (sem janela, sem X11, sem mexer na tela do dono). Cobertos: abrir o cofre e listar no explorador; abrir nota pelo Abrir rápido (Ctrl+O); editar, Ctrl+S com bytes exatos e desfazer; leitura que não aceita digitação; busca lateral (Ctrl+Shift+F) com e sem resultado; painel de links de entrada nas duas direções.
Achado do harness: o editor de texto não tem nome acessível (desligado com `with_accessibility_check(false)` até alguém decidir se vale consertar).

## R8b: o que falta para `git ls-files '*.py'` ficar vazio
Sobram 13.343 linhas, todas dirigidas por X11 (`xdotool`, `Xvfb`, Pillow) ou presas a elas:
- `tests/gui_smoke.py` (11.852 linhas, ~120 seções): migrar seção a seção para o `egui_kittest`, começando pelas que não dependem de pixel nem de janela real (palette, undo, tabs, tabelas, tarefas, backlinks com contexto, workspaces, recovery, settings, bookmarks, navegação). Seções que dependem do sistema (system-open, system-theme, instance, reveal, vault-switch com seletor nativo, agent-terminal) ficam por último, ou viram teste de integração do crate, ou saem se a função já tiver cobertura.
- `scripts/native_input.py`, `scripts/record-keyboard.py` (ajudantes do X11), `tests/vault_loading_checks.py` e `tests/agent_terminal_checks.py` (módulos do gui_smoke): saem junto com ele.
- `scripts/compare-editors.py`: comparação externa com o editor Ferrite por xdotool; decidir se ainda interessa (se sim, vira medição Rust sem X11; se não, apagar).
- `tests/emoji_font_asset.py` (FontTools/HarfBuzz): as conferências de hash das partes podem virar teste Rust; a de shaping depende de HarfBuzz.
- `experiments/html-host/verify.py`: experimento CEF abandonado; decidir se apaga.
- Depois disso, tirar `python3`/`xvfb`/`xdotool` dos workflows `identity.yml`, `ux.yml`, `syntax.yml` e trocar `gui_smoke` por `cargo test`.

### R8b-1 (17/11): 38 seções migradas para o egui_kittest
Os testes de GUI viraram a pasta `src/app/gui_tests/` (ajudantes em `mod.rs`, `edicao.rs` com 42 testes, `navegacao.rs` com 74). `cargo test --bin rustidian`: 205 testes, 1 ignorado, verde 3 vezes seguidas no Windows (~100 s).
- **Edição:** undo, list-indent, list-enter, delimiter-pairs, unicode-queued, tasks, tables, table-input, table-links, table-edit, folding, inline-styles (parcial), callouts, note-aids, external-rename, block-completion.
- **Navegação:** tab-state, tab-actions, tab-restore, tab-close-menu, navigation, quick-create, quick-ranking, bookmarks, workspaces, note-dialogs, folders, folder-create, file-copy, folder-copy, file-move, trash-options, explorer, palettes, settings, recovery, backlink-context, link-fragments.
- **Ficou de fora de propósito:** medição de pixel/cor/geometria, arrastar com mouse real, seletor nativo, reinício do processo, `chmod`, lixeira do sistema (os testes nunca usam), botão de dobra da leitura (sem nome acessível).
- **Achados:** o harness não chama `raw_input_hook` (Tab/Enter enfileirados no mesmo quadro se perdiam; `edicao.rs` tem `Session::frame()` e `queued()`). **Bug do app no Windows:** renomear/mover pasta com subpastas dá "Acesso negado" com o vigia de arquivos ligado (teste `renomear_pasta_com_subpastas_com_o_vigia_ligado` com `#[ignore]`).
- **Ainda não feito:** tirar essas seções do `gui_smoke.py` e das listas dos workflows. Primeiro ver o `cargo test` passar no CI Linux (cópia de pasta e mover pasta têm ramo por sistema).

## R7a: protótipos Android (egui × Dioxus)

Pergunta que decide o caminho: **qual toolkit digita bem pelo Gboard** (acentos, autocorretor, sugestões, apagar, selecionar com o dedo, rolar, teclado sem cobrir o texto). Dois protótipos mínimos em `prototipos/android-egui` e `prototipos/android-dioxus`, fora do workspace (`exclude` no `Cargo.toml` da raiz e `[workspace]` vazio em cada um; o gate `cargo test --workspace` do app não os vê). Tela igual nos dois: campo multilinha grande, botão Salvar (grava `nota.md` na pasta interna do app) e rótulo de status. Roteiro do dono: `docs/bruno/R7-TESTE.md`. Não houve emulador nem aparelho neste PC: o que se diz abaixo sobre o comportamento no celular vem do teste do dono ou é inferência marcada como tal.

### O que funcionou no build
- **Ferramentas instaladas (além de JDK 21, SDK e NDK):** `cargo-ndk` 4.1.2, `dioxus-cli` 0.7.10 (`dx`), `cargo-apk` 0.10.0 (instalado, **não usado**) e o Gradle 8.14.3 em `C:\dev\_ferramentas\gradle-8.14.3` (distribuição oficial; o projeto do egui usa AGP 8.13 e `androidx.games:games-activity:4.4.0`).
- **egui:** `eframe =0.37.0` com `glow` + `android-game-activity`. `cargo ndk -t arm64-v8a -P 26 ... build --release` gera `librustidian_proto_egui.so` (5 MB; só liga em `liblog`, `libandroid`, `libdl`, `libm` e `libc`, sem `libc++_shared`), e o Gradle empacota com `GameActivity` como `MainActivity`. APK ~4,8 MB, `minSdk 26`, `targetSdk 34`, arm64-v8a, assinado com a chave de debug do Android (`signingConfig signingConfigs.debug`). `prototipos/android-egui/construir.sh` repete o processo.
  - `targetSdk 34` de propósito: a partir do 35 o Android força tela de ponta a ponta e ignora `adjustResize`, o que faria o teclado cobrir o texto.
  - **NativeActivity (cargo-apk) descartada:** o README da `android-activity` 0.6.1 diz que ela só dá texto básico, "unlikely to be sufficient for production applications". Texto composto exige GameActivity, e esta exige Gradle (a classe Java vem de um AAR do AndroidX).
- **Dioxus:** 0.7.10 (estável; a 0.8 ainda é alfa). `dx bundle --android --release --target aarch64-linux-android --features mobile --package-types apk` gerou o APK (~12,6 MB, `minSdk 24`, `targetSdk 34`, arm64-v8a, chave de debug). Primeira compilação ~19 min neste PC (465 crates e o Gradle, que o `dx` baixa sozinho). O `dx` gera o projeto Gradle e os arquivos Kotlin da WebView (wry/tao). O `<textarea>` é **não controlado** (sem `value` reescrito pelo Rust a cada tecla), senão o cursor pula e a composição do teclado quebra.
- Testes no PC: `cargo test --lib` do egui, 17 testes (tradução do IME e filtro de toque). O gate do app não foi alterado.

### O que se sabe do IME de cada toolkit
**egui / winit 0.30 (o do eframe 0.37).** No Android o winit só liga e desliga o teclado: `set_ime_allowed` chama `show_soft_input`/`hide_soft_input`; `set_ime_cursor_area` e `set_ime_purpose` não fazem nada; e ele **nunca emite `WindowEvent::Ime`**. O `TextEvent` que o GameActivity entrega cai no ramo `_ => warn!("Unknown android_activity input event")` (fonte: `winit-0.30.13/src/platform_impl/android/mod.rs`). Sem `Ime`, o egui só recebe teclas físicas: acento composto, autocorretor e sugestões não chegam. O suporte de IME no Android é trabalho em aberto no winit: PR rascunho [winit#4451](https://github.com/rust-windowing/winit/pull/4451) (IME básico no Android, só GameActivity, apoiado em [android-activity#214](https://github.com/rust-mobile/android-activity/pull/214)); o winit 0.31 tem uma API de IME mais difícil de mapear no Android porque não altera o texto em volta, só apaga; e o PR [winit#3787](https://github.com/rust-windowing/winit/pull/3787) já notava que o egui espera `KeyEvent::text` preenchido. O egui 0.37 já tem `ImeEvent::{Preedit, Commit, DeleteSurrounding}` no `TextEdit`; só falta o backend entregá-los.
- **Conserto feito no protótipo (sem mexer no winit):** `src/ime.rs` lê `AndroidApp::text_input_state()` a cada ~30 ms enquanto o editor tem foco, compara com o estado anterior e injeta `Preedit`/`DeleteSurrounding`/`Commit` no egui. O teclado é configurado com `set_ime_editor_info` (texto livre, multilinha, autocorreção). Custo: sondagem em vez de evento (acorda o app a cada 30 ms com o teclado aberto) e o cursor/seleção do Gboard não são espelhados (o app zera a memória do teclado a cada toque no campo).
- Pegadinhas que o código já trata: o `TextEdit` **ignora `Commit` de quebra de linha** (ela tem de entrar como tecla Enter) e os índices do GameTextInput vêm em bytes do UTF-8 modificado do Java (emoji ocupa 6 bytes, não 4).

**Dioxus (renderer mobile = wry/tao + WebView do Android).** O texto é editado pelo próprio motor do Chrome (`<textarea>`): o Gboard conversa com a WebView como em qualquer página, então acento, autocorretor, sugestões, lupa e alças de seleção vêm prontos. Não achei issue específico de Dioxus sobre textarea no Android; a conclusão é inferência (mesmo mecanismo das páginas no Chrome) e depende do teste do dono. Pontos de atenção conhecidos de WebView: o teclado cobrir o campo (mitigado com `interactive-widget=resizes-content` e `height: 100dvh`; o `dx` não expõe `windowSoftInputMode`) e a composição quebrar se o Rust reescrever o valor.

### Resultado do primeiro teste do dono (egui v1, Gboard)
Acentos funcionaram (a tradução do IME serve). Falhas: (1) Enter não pulava linha; (2) tocar e arrastar o cursor selecionava texto sem toque longo e "travava" na seleção, ou seja, o dedo virava mouse; (3) emoji do Gboard não aparecia.

**Consertos da v2 (`rustidian-proto-egui-v2.apk`, ainda não testada no aparelho):**
1. **Enter:** o `Commit` de quebra de linha agora vira a tecla Enter (pressionar e soltar), e a ação do teclado passou a `TextInputAction::None` (a doc da `android-activity` diz que o receptor pode tratar como Enter que insere linha nova). Se o Enter estiver chegando como "ação do editor" (`InputEvent::TextAction`), o winit também o descarta e a sondagem não o vê; por isso a linha de diagnóstico mostra a última tecla recebida, para o dono relatar.
2. **Toque:** `src/toque.rs` reescreve os eventos brutos em `App::raw_input_hook`: toque curto = cursor; arrastar mais de 10 pt = rolar (`MouseWheel`); parado por 0,45 s = duplo clique sintético (seleciona a palavra, e arrastar em seguida estende). Sem alças de seleção, sem lupa e sem inércia: ficam fora do protótipo.
3. **Emoji (pergunta sobre a fonte do sistema):** dá para carregar a fonte do sistema, e o egui 0.37 desenha emoji colorido **se** o `epaint` for compilado com `color_fonts` (COLR, sbix e CBDT/EBDT via skrifa; não vem ligado por padrão) e a fonte entrar como reserva. A v2 tenta `/system/fonts/NotoColorEmoji.ttf`, `SamsungColorEmoji.ttf` e `ColorEmoji.ttf` (o Android permite ler `/system/fonts`), põe a primeira achada logo depois da fonte principal e escreve o resultado na linha de diagnóstico. **Não testado em aparelho:** falta confirmar que sai colorido e que emoji de sequência (bandeira, família) apagam inteiros. A fonte tem ~10 MB e é lida inteira na memória ao abrir. Sem ela, o egui só mostra o emoji de uma cor que vem embutido.

### Palpite
**O Dioxus deve ganhar na digitação e na sensação de edição** (alças de seleção, lupa, menu copiar/colar, rolagem com inércia, emoji, ditado: tudo do motor do Chrome). O egui só chegou ao mínimo (acentos) depois de duas pontes escritas à mão (IME e toque), e ainda sem alças nem lupa. O argumento a favor do egui é reaproveitar o app inteiro (renderização de Markdown, abas, tela de sync, patches de bidi); mas o editor do Rustidian é um `TextEdit` do egui, e é justamente ele que não tem experiência móvel pronta. Recomendação provisória: **no Android, UI em Dioxus (ou WebView) e o miolo em Rust (`nota-cripto`, `nota-sync`) reaproveitado como biblioteca**; o egui só volta à mesa se a v2 passar nos passos 3 a 10 do roteiro e o dono achar a sensação aceitável. A decisão final sai do teste do dono com `rustidian-proto-dioxus.apk` e `rustidian-proto-egui-v2.apk`.

### Decisão do dono (18/11/2026)
**Fica o Dioxus.** No teste no celular dele o `rustidian-proto-dioxus.apk` se saiu melhor que o egui v2. O app Android (R7b) sai em Dioxus, com o miolo em Rust (`nota-cripto`, `nota-sync`) como biblioteca. Referência para montar o Android: o SoloMD (MIT, Tauri 2, já na Play Store; ver `COMPARACAO-2026-11.md`), o plano B se o caminho do Dioxus emperrar. Do Onyx (AGPL) só ideias, nenhum código.

### Riscos e pendências
- O Dioxus ainda não foi executado em lugar nenhum: só se sabe que compila e empacota. A pasta `/data/data/dev.rustidian.protodioxus/files` é escrita pelo caminho fixo do pacote.
- O APK do Dioxus sai da variante `debug` do Gradle do `dx` (com o Rust em `--release`) e é assinado com a chave de debug do Android, a mesma do egui. Não há keystore de release.
- A sondagem a cada 30 ms do egui gasta bateria com o teclado aberto; a solução de verdade é um evento (winit#4451) ou um loop de eventos próprio.
- Outros teclados (Samsung, SwiftKey) estão fora desta fatia.

## R16: assinatura das atualizações (feito, falta a chave do dono)

Só a ideia veio do opensidian (SHA256SUMS assinado, chave pública no app); o código é nosso e do zero. **Não existe auto-update nem rede nova**: é só a conferência local do que o dono baixou.

**Desenho.** O release tem `SHA256SUMS` (formato do `sha256sum`) e `SHA256SUMS.sig` (assinatura ed25519 da lista, texto fixo `rustidian-release-v1\n` + a lista, em hexadecimal). O app embute só a chave pública (`src/atualizacao/chave-publica.txt`, `include_str!`; hoje diz `NENHUMA`). O módulo `src/atualizacao.rs` serve ao app e ao `xtask` (incluído por `#[path]`), então quem assina e quem confere usam o mesmo formato.
- **Crate:** `ed25519-dalek` 2.2 (BSD-3-Clause, compatível com MIT; Rust puro, assina e verifica, do RustCrypto/dalek, a mais usada), com `default-features = false`; `sha2` já estava no lock. Fica de fora a `minisign-verify` (só verifica, e não teríamos o `minisign` para assinar).
- **`cargo xtask release --dir PASTA [--chave ARQUIVO]`:** gera `SHA256SUMS` de todos os arquivos da pasta e assina. A chave vem de `--chave` ou do CAMINHO em `RUSTIDIAN_CHAVE_PRIVADA` (nunca a chave em argumento ou variável). Sem chave: o release sai, avisa em voz alta e apaga qualquer `.sig` velho. Recusa uma chave que não seja a par da pública embutida, e confere o resultado do mesmo jeito que o app.
- **`cargo xtask package-linux ... [--chave ARQUIVO]`:** põe `SHA256SUMS` (+ `.sig`) dentro do tar.gz, cobrindo todos os arquivos do pacote.
- **`rustidian --install`:** antes de copiar qualquer coisa confere a assinatura e o hash de todos os arquivos da pasta do pacote, e recusa arquivo que esteja na pasta mas não na lista (ninguém planta um arquivo que o instalador copiaria), link simbólico, caminho com `..`. Falhou: mensagem em pt-BR, exit 2, nada instalado. `--sem-assinatura` (só desenvolvedor) pula a conferência e avisa.
- **`rustidian --verificar PASTA_OU_ARQUIVO`:** confere um download (a lista assinada fica na mesma pasta) sem instalar nada; funciona no Windows também. Não criei `--atualizar`: o app não extrai tar.gz nem baixa nada, então "atualizar" seria só instalar; o caminho é verificar → extrair → `./rustidian --install` (que confere de novo).
- **Limites conhecidos:** a assinatura não carrega número de versão, então um pacote antigo (assinado) ainda passa (sem proteção contra voltar de versão); trocar a chave invalida tudo que já saiu (`chave-nova` exige `--trocar` quando já há chave embutida). Os pacotes feitos pelo CI não têm a chave privada e saem sem assinatura: até existir um segredo no Actions (decisão do dono), o tar.gz do CI só instala com `--sem-assinatura`, ou o dono assina o pacote no PC.

**Passo a passo do dono (ele roda; nenhuma chave real foi gerada):**
1. Escolher uma pasta FORA do repositório para a chave, por exemplo `C:\Users\<voce>\.rustidian\assinatura.chave` (a pasta precisa existir).
2. No repositório: `cargo xtask chave-nova C:\Users\<voce>\.rustidian\assinatura.chave`. Ele escreve a privada nesse arquivo (e não imprime) e troca a linha de `src/atualizacao/chave-publica.txt`.
3. Guardar uma cópia da privada num lugar seguro (gerenciador de senhas ou pen drive). Se perder, é preciso gerar outra chave e publicar um app novo; se vazar, qualquer um assina por ele.
4. Fazer commit só de `src/atualizacao/chave-publica.txt` e recompilar o app (a chave é embutida na compilação).
5. A cada release: `cargo xtask release --dir <pasta dos arquivos do release> --chave C:\Users\<voce>\.rustidian\assinatura.chave`, e publicar `SHA256SUMS` e `SHA256SUMS.sig` junto. Para o pacote portátil assinado: `cargo xtask package-linux --binary ... --output ... --chave <arquivo>`.
6. Quem baixa: `rustidian --verificar <pasta dos downloads>` antes de instalar.

## O que depende do dono
- **R6:** login no Google pelo `rclone config` (no re-servidor, com o navegador do PC), e criar e guardar as duas senhas.
- **R7:** ok para instalar o Android SDK e o NDK (~5–8 GB) no PC; instalar o APK no celular.
- Escolher a frase-senha do cofre (só ele digita).
- **R16:** gerar a chave de assinatura dele (passo a passo na seção R16) e decidir se o Actions ganha um segredo para assinar o pacote do CI.

## Contribuir com o original (MasihMoafi/rustidian)

Um fork só: este. Há dois tipos de branch.
- **`main` (nosso produto):** Windows, criptografia, sync, Android e backup. O que é pessoal (servidor do Bruno, rclone) fica aqui e não sobe para o original.
- **`upstream/<assunto>`:** correções gerais que servem a todos, como a imagem remota opcional, o histórico configurável, o suporte a Windows e os testes em Rust. Cada uma nasce do `main` do original (`git remote add original https://github.com/MasihMoafi/rustidian`), tem um commit limpo em inglês e vira Pull Request no original. **Abrir PR é publicar: só com o ok do dono para cada um.**
- **Puxar novidades do original:** `git fetch original && git merge original/main` no nosso `main`, com o gate rodando depois. Quanto mais o nosso código ficar em crates separadas e atrás de `cfg`, menos conflito.
