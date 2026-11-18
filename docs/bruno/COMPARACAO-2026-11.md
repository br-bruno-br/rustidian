# Comparação de projetos tipo Obsidian (18/11/2026)

Pergunta: a base que escolhemos (o fork do Rustidian) continua sendo a melhor para o que o Bruno quer, ou outro projeto aberto é melhor?

**O que ele quer** (do PLANO): Windows + Android, cofre de Markdown puro compatível com Obsidian (`[[links]]`, backlinks, tags, tarefas), sincronização cifrada no próprio servidor (re-servidor), backup, corretor pt-BR, rápido, de preferência Rust, e nada se perde (na dúvida guarda as duas versões).

Dados de estrelas, licença, último commit e releases vieram da API do GitHub em 18/11/2026. O que veio de leitura de README ou de busca na web está marcado como "(README)" ou "(web)". Nada foi instalado nem testado em aparelho.

## Resposta curta

**Manter o fork do Rustidian.** Nenhum projeto aberto junta, hoje, as quatro coisas que importam: cofre de arquivos Markdown, Windows + Android funcionando, sync cifrado no servidor dele com "guarda as duas versões" e corretor pt-BR. O que mais chega perto (SoloMD) tem Android e Windows prontos, mas o corretor só entende inglês e o sync é um conjunto de adaptadores (WebDAV, S3, Git), não um servidor próprio com protocolo testado. O que tem o desenho mais parecido com o nosso (Onyx) parou em julho.

**O ponto fraco do fork é o Android, e isso não depende de qual base ele usa.** O egui no Android não recebe o teclado composto do Gboard sem remendo (ver R7a no PLANO). Os três projetos modernos que rodam em Android (SoloMD, Onyx, Tolaria no iOS) usam Tauri com editor CodeMirror dentro de uma WebView, o mesmo caminho que o PLANO já aponta como provável (Dioxus/WebView por cima das crates `nota-cripto` e `nota-sync`). Trocar de base não evita esse trabalho: o miolo em Rust seria reaproveitado de qualquer jeito.

## Tabela

| Projeto | Stack | Licença | Estrelas | Atividade | Android | Cofre MD puro (Obsidian) | Sync cifrado no próprio servidor | Plugins | Risco principal |
|---|---|---|---|---|---|---|---|---|---|
| **Rustidian (fork nosso)** | Rust, egui | MIT | 19 (original) | original criado em 20/10/2026, 303 commits, 1 autor; release 0.1.6 em 04/11 | Não (a fazer, R7) | Sim | Sim, nosso (`nota-sync` + `nota-servidor`, testes de propriedade) | Não | Original pequeno e de um autor só; egui sem teclado composto no Android |
| **SoloMD** | Tauri 2, Vue, CodeMirror 6, Rust | MIT | 1.658 | v5.2.3 em 18/11; criado em 08/04/2026; 808 de ~850 commits de um autor, escrito quase todo por Claude (README) | Sim, Google Play e APK | Sim (`[[links]]`, backlinks, frontmatter) | Parcial: E2EE (AES-GCM / XChaCha20) sobre WebDAV, S3, Git, nuvens; conflito vira "cópia de conflito" | JSON declarativo, sem JS | Um autor; corretor só en_US e só letras A-Z (não pega acento); interface e comunidade em chinês |
| **Onyx** (onyx-notes) | Rust + Tauri 2, SolidJS, CodeMirror, servidor axum | AGPL-3.0 | 5 | criado em 17/07/2026, **último commit em 18/07** (4 meses parado) | Tem projeto Android gerado; correções de Android em julho; sem release | Sim, abre cofre do Obsidian "no lugar" | Sim: servidor de conhecimento zero, CRDT (Loro), backup cifrado em S3/Drive/WebDAV | Sim (iframe isolado) | Parado, 5 estrelas, sem release; AGPL |
| **Tolaria** | Tauri, React, TypeScript | AGPL-3.0 | 20.581 | releases alfa todos os dias (18/11); empresa por trás | **Não** (iOS em beta; desktop Win/Mac/Linux) | Sim, cofre = repositório Git | Git (qualquer remoto); sem E2EE próprio | Não (MCP/IA) | Sem Android; sync é Git (sem cifra) |
| **Logseq** | Clojure/ClojureScript | AGPL-3.0 | 45.650 | 2.0.2 beta em 07/10/2026; 2.0.1 em 13/07 | Sim, mas app móvel e sync RTC em alfa (web) | Parcial: formato em lista com `-`; versão nova usa banco, não arquivos | Sync RTC em alfa, sem relato de E2EE próprio no servidor dele | Sim | A versão nova "pode perder dados" (aviso deles, web); migração de arquivos para banco |
| **SiYuan** | Go + TypeScript (Electron) | AGPL-3.0 | 47.279 | RC de 17/11; Android 3.7.3 em jul/2026 (F-Droid, web) | Sim | Não: formato próprio (`.sy`), Markdown só importa/exporta (web) | E2EE no sync, mas sync oficial é pago; S3/WebDAV é PRO (web) | Sim | Formato fechado para o Obsidian; recursos pagos |
| **Joplin** | TypeScript (Electron / React Native) | AGPL-3.0 (mista) | 57.233 | v3.7.22 em 28/10 | Sim, nativo | Não: notas em banco, não em cofre de arquivos | Sim: E2EE; sync por Joplin Server, WebDAV, S3 | Sim, muitos | Não é cofre de arquivos; backlinks fracos |
| **Notesnook** | TypeScript (React Native + web) | GPL-3.0 | 14.953 | releases em 16/11 | Sim, nativo | Não: banco próprio | Sim: E2EE, servidor próprio existe (AGPL, C#, 956 estrelas) | Limitado | Empresa por trás; não é cofre de arquivos |
| **Standard Notes** | TypeScript | AGPL-3.0 | 6.726 | último push 13/11 | Sim, nativo | Não | Sim: E2EE, servidor próprio | Extensões (parte paga) | Proton comprou e fala pouco do futuro (web) |
| **Anytype** | TypeScript + Go | Código **disponível**, não aberto (Any Source Available License) | 9.114 | push em 10/11 | Sim | Não: objetos, não arquivos | Sim: any-sync (MIT) com servidor próprio | Não | Licença que não é aberta; formato próprio |
| **Trilium (TriliumNext)** | TypeScript | AGPL-3.0 | 38.989 | v0.107.0 em 08/11 | Não oficial (app de terceiros) | Não: banco SQLite | Servidor próprio, sem E2EE no sync | Sim (scripts) | Sem app Android oficial; formato próprio |
| **Zettlr** | TypeScript (Electron) | GPL-3.0 | 13.933 | v4.10.0 em 13/11 | **Não** | Sim, arquivos Markdown | Nenhum | Não | Sem celular nem sync |
| **AppFlowy** | Dart (Flutter) + Rust | AGPL-3.0 | 79.138 | 0.15.1 em 15/11 | Sim | Não: banco / blocos | Nuvem própria (self-host), sem E2EE | Sim | Foco em equipe estilo Notion; não é cofre de arquivos |
| **SilverBullet** | servidor Rust + cliente TypeScript | MIT | 6.555 | commit em 18/11 | PWA no navegador (sem app nativo) | Sim, arquivos Markdown numa pasta | Sem E2EE (o servidor lê as notas); roda no re-servidor, atrás da Tailscale | Sim (Lua) | Precisa do servidor no ar; não é app nativo |
| **Opensidian** | Rust + Tauri 2 | GPL-3.0 | 396 | v0.9.5 em 17/11; criado em 29/08 | Pacotes só para Linux (README) | Sim | Não encontrado | Não encontrado | Muito novo, 99 pendências, só Linux |
| **Lokus** | Rust + Tauri | sem licença clara no GitHub | 844 | alfa de 18/11 | Não (README) | Sim | Relay de equipe; E2EE "ainda não" | Sim | Virou ferramenta de equipe/CRM; alfa |
| **Reor** | Electron | AGPL-3.0 | ~8.400 (web) | **arquivado em 07/03/2026** (web) | Não | Sim | Não | Não | Morto |

Fora da tabela: **AFFiNE** (74.637 estrelas, licença mista e Notion-like, não é cofre de arquivos) e **Obsidian** em si (fechado, sync pago). Dois reservas que não são projetos de notas: **Syncthing** (MPL-2.0, 90.560 estrelas, atividade em 17/11) e **Markor** (Android, 6.443 estrelas, edita arquivos Markdown): servem para uma saída de emergência simples, "Markor no celular + Syncthing", sem E2EE no servidor e sem backlinks bons.

## Os 3 melhores para o que ele quer

1. **Fork do Rustidian (manter).** É o único com o cofre de arquivos, sync cifrado com protocolo nosso, testes de propriedade para conflito, Windows já funcionando e corretor pt-BR já feito. Falta o Android, que é falta em qualquer caminho.
2. **SoloMD.** É o único aberto (MIT) que já entrega Windows + Android com cofre Markdown, wikilinks e backlinks, E2EE e cópia de conflito. Serve de referência concreta para o Android e, na pior hipótese, de plano B. Não serve como base hoje por causa do corretor (só inglês, e a expressão `[A-Za-z]` ignora palavras acentuadas) e do sync por adaptadores.
3. **Onyx.** Tem o desenho mais parecido com o nosso (servidor de conhecimento zero em Rust, CRDT, backup cifrado, abre cofre do Obsidian sem mexer), mas está parado desde 18/07 com 5 estrelas e sem release. Vale como fonte de ideias, não como base.

Tolaria é o mais vivo e popular, mas não tem Android e o sync é Git sem cifra: fica fora.

## Trocar ou manter: o custo honesto

**Manter (recomendado).** Custo: terminar o Android (R7), que já era necessário. Risco real: o original do Rustidian é de um autor só, tem 4 semanas de vida e 19 estrelas. Mitigação: o fork já tem o que importa em crates separadas (`nota-cripto`, `nota-sync`, `nota-servidor`) que não dependem do original, então se o original parar de evoluir só perdemos as correções do editor egui, que mantemos nós.

**Trocar para SoloMD (não recomendado agora).** Ganho: Android e Windows em dias. Custo: perder o egui, o corretor pt-BR já pronto, e o protocolo de sync com teste de propriedade, e passar a depender de um autor único (também) que trabalha em chinês e com revisão feita por IA. Teria de refazer o corretor para pt-BR e ainda ligar o sync do re-servidor via WebDAV. Seria trocar um risco de autor único por outro, mais refazer R3 a R5 no desenho dos outros.

**Trocar para Onyx.** Mesmo desenho, mas projeto parado: herdaríamos código AGPL sem mantenedor. Custo igual ao de refazer o que já fizemos.

**Gatilho para reabrir a decisão:** se o teste do Dioxus/Tauri no Android (R7) mostrar que o miolo Rust é pequeno demais para valer o app em volta, ou se SoloMD ganhar corretor com dicionário Hunspell por idioma (sem filtro A-Z) e um servidor próprio de sync. Reavaliar em fevereiro/2027.

## O que vale copiar dos outros

Atenção à licença: nosso fork é MIT. Código de AGPL/GPL (Onyx, Tolaria, Logseq, SiYuan, Opensidian) **não pode** ser copiado sem tornar o fork AGPL/GPL; só ideias. Código MIT/Apache pode, com crédito em `THIRD_PARTY_NOTICES.txt`.

| De onde | O que copiar | Licença | Link |
|---|---|---|---|
| SoloMD | Como o Tauri 2 Android é montado (editor CodeMirror 6 numa WebView, teclado Gboard funcionando, APK/AAB e publicação na Play). É a maior economia para o R7 | MIT (pode copiar código) | [repositório](https://github.com/zhitongblog/solomd), [`app/src-tauri`](https://github.com/zhitongblog/solomd/tree/main/app/src-tauri) |
| SoloMD | Nome da cópia de conflito com hash curto do conteúdo (`<nome> (conflito <origem> <data>_<sha6>)`), e manifesto remoto com SHA-256 para pular arquivo que não mudou | MIT | [`webdav_sync.rs`](https://github.com/zhitongblog/solomd/blob/main/app/src-tauri/src/webdav_sync.rs) |
| SoloMD | AutoGit: um commit local por Ctrl+S como histórico por nota (libgit2); nosso histórico de recuperação hoje é de 1 dia | MIT | [README](https://github.com/zhitongblog/solomd#readme) |
| SoloMD | Servidor MCP embutido para o Claude ler o cofre | MIT | [`mcp-server`](https://github.com/zhitongblog/solomd/tree/main/mcp-server) |
| Onyx | Mesclar texto com CRDT (Loro) em vez de guardar as duas versões: evita "(conflito)" quando duas edições não se tocam. Só ideia (AGPL) | AGPL (só ideia) | [`crates/onyx-sync`](https://github.com/onyx-notes/onyx/tree/main/crates/onyx-sync) |
| Onyx | Cifrar também o nome do arquivo, índices só em memória, pareamento de aparelho por código, "edição vence exclusão", teto de desempenho medido em cofre de 100 mil notas no CI | AGPL (só ideia) | [README](https://github.com/onyx-notes/onyx#readme) |
| Onyx | Backup cifrado e deduplicado por OpenDAL (S3, WebDAV, disco). Hoje o plano é rustic + rclone; o OpenDAL pode tirar o rclone | AGPL (só ideia); o OpenDAL é Apache-2.0 | [OpenDAL](https://github.com/apache/opendal) |
| RustSidian | Menções sem link (já copiado, ideia) | Apache-2.0 | [repositório](https://github.com/siddarth1872004/RustSidian) |
| Tolaria | Cofre como repositório Git com histórico visual e merge no celular; só como ideia para um futuro "histórico de uma nota" | AGPL (só ideia) | [repositório](https://github.com/refactoringhq/tolaria) |
| SilverBullet | Servidor Rust que serve o cofre pela web: pode ser uma "janela de emergência" para ler as notas no celular sem instalar nada, atrás da Tailscale | MIT | [repositório](https://github.com/silverbulletmd/silverbullet) |
| Logseq | Nada a copiar; vale só o aviso: não usar a versão 2.0 em banco como cofre por causa do risco de perda de dados | AGPL | [fórum](https://discuss.logseq.com/t/whats-new-with-logseq-db-may-16th-2026/35020) |

Os dois itens de maior retorno: (1) o modelo Tauri-Android do SoloMD para o R7, e (2) CRDT de texto (Loro) como melhoria futura do sync para reduzir cópias de conflito.

## Pontos que ainda precisam de conferência

- Android do SoloMD e do Onyx: só li README e código; ninguém testou em aparelho. Vale instalar o APK do SoloMD (Play ou [releases](https://github.com/zhitongblog/solomd/releases/latest)) **numa cópia do cofre, nunca no cofre real**, e testar digitação com acento no Gboard. Isso também dá a régua para o R7.
- SoloMD em português: o README existe em português, mas o corretor é só `en_US` ([`spellcheck.rs`](https://github.com/zhitongblog/solomd/blob/main/app/src-tauri/src/spellcheck.rs)). Não vi a interface em pt-BR.
- AFFiNE, Anytype (termos exatos da licença), Logseq (estado de E2EE) e SiYuan (formato em disco) vêm de busca na web e README, sem leitura do código.
- Estrelas de projetos que mudam rápido (Tolaria, SoloMD) variam de dia para dia.
- Alguns projetos novos têm estrelas inflacionadas ou foram escritos por IA; estrela não prova que o sync não perde dados.

## Fontes

Estatísticas: API do GitHub (`gh api repos/<dono>/<repo>`) em 18/11/2026.

- Rustidian original: https://github.com/MasihMoafi/rustidian
- SoloMD: https://github.com/zhitongblog/solomd · site https://solomd.app
- Onyx: https://github.com/onyx-notes/onyx
- Tolaria: https://github.com/refactoringhq/tolaria · site https://tolaria.md · [artigo de lançamento](https://refactoring.fm/p/introducing-tolaria)
- Logseq: https://github.com/logseq/logseq · [novidades da versão DB](https://discuss.logseq.com/t/whats-new-with-logseq-db-may-16th-2026/35020) · [releases](https://github.com/logseq/logseq/releases)
- SiYuan: https://github.com/siyuan-note/siyuan · [F-Droid](https://f-droid.org/pt_BR/packages/org.b3log.siyuan/)
- Joplin: https://github.com/laurent22/joplin
- Notesnook: https://github.com/streetwriters/notesnook · servidor https://github.com/streetwriters/notesnook-sync-server
- Standard Notes: https://github.com/standardnotes/app · [anúncio Proton](https://proton.me/pl/blog/proton-standard-notes-join-forces) · [balanço 2025](https://discuss.privacyguides.net/t/standard-notes-2025-update-what-s-new-and-whats-next/33774)
- Anytype: https://github.com/anyproto/anytype-ts · [filosofia aberta](https://blog.anytype.io/our-open-philosophy/)
- Trilium: https://github.com/TriliumNext/Trilium
- Zettlr: https://github.com/Zettlr/Zettlr
- AppFlowy: https://github.com/AppFlowy-IO/AppFlowy
- AFFiNE: https://github.com/toeverything/AFFiNE
- SilverBullet: https://github.com/silverbulletmd/silverbullet
- Opensidian: https://github.com/jpzk/opensidian
- Lokus: https://github.com/lokus-ai/lokus
- RustSidian: https://github.com/siddarth1872004/RustSidian
- Reor (arquivado): https://github.com/reorproject/reor
- Markor: https://github.com/gsantner/markor · Syncthing: https://github.com/syncthing/syncthing
- Listas de alternativas consultadas: [openalternative.co](https://openalternative.co/alternatives/obsidian), [alternativeto.net](https://alternativeto.net/software/obsidian/?license=opensource&platform=android)

## Os três projetos em Rust que o Bruno mandou (18/11)

Pergunta: dos três projetos "não oficiais" em Rust que o Bruno mandou em 16/11, algum é melhor que o nosso fork, e o que ainda vale trazer de cada um? Dados da API do GitHub em 18/11/2026 (estrelas, datas, licença, linguagens, commits) e leitura de README, `FEATURES.md`, `ROADMAP.md` e lista de arquivos. Nada foi instalado nem rodado; "testes" abaixo é o que o próprio projeto declara ou o que a lista de arquivos mostra.

### Estado de cada um

| | [Hu-Tao128/rustidian](https://github.com/Hu-Tao128/rustidian) | [jpzk/opensidian](https://github.com/jpzk/opensidian) | [siddarth1872004/RustSidian](https://github.com/siddarth1872004/RustSidian) |
|---|---|---|---|
| Interface | Slint (software renderer, feito para PC velho) | Tauri 2 + JavaScript (WebView) | Interface própria em Rust puro (sem toolkit) |
| Licença | MIT | **GPL-3.0** | MIT **ou** Apache-2.0 (os dois arquivos de licença estão no repositório) |
| Estrelas | 2 | 396 | 1 |
| Criado em | 25/09/2026 | 29/08/2026 | 12/11/2026 (6 dias) |
| Último commit | **21/10** (4 semanas parado) | 18/11 (hoje) | 16/11 |
| Ritmo nos últimos 7 dias | 0 commits | pelo menos 100 (a consulta corta em 100); release v0.9.5 em 17/11 | 78 commits; releases 0.6.0 e 0.6.1 em 16/11 |
| Autoria | 1 pessoa, código e comentários em espanhol | 1 mantenedor; o README diz que quase tudo foi escrito pelo Claude | 1 pessoa |
| Tamanho (bytes de Rust) | ~200 KB | ~1,8 MB (e 2,4 MB de JavaScript, boa parte de bibliotecas embutidas) | ~1,65 MB, em 11 crates |
| Testes | testes só no `core` (links, vault, Markdown); UI sem teste | pasta `src-tauri/tests` com fixtures; 99 pendências abertas | declara 251 testes, inclusive de propriedade (`proptest`) no editor e no Markdown, e medição de desempenho |
| Plataformas | Windows, Linux, macOS. **Sem Android** | **Só Linux** nos pacotes do README (AppImage, Flatpak, RPM, DEB). Sem Windows nem Android | Windows, macOS, Linux. **Sem Android** (está no roteiro deles como futuro) |
| Sync | Nenhum | Nenhum encontrado no README | **Nenhum.** O `ROADMAP.md` lista "sync cifrado com servidor próprio" e app Android como metas futuras |
| Cofre compatível com Obsidian | Pasta de `.md` simples, `[[links]]`, backlinks; não lê a pasta `.obsidian` | Sim, é o objetivo ("clone de comportamento"); lê temas e plugins do `.obsidian` | Sim, abre cofre do Obsidian como está; guarda os dados dele em `.rustsidian/` |
| Corretor / idioma | Sem corretor | Não vi corretor no README | Corretor só **en_US** (dicionário embutido); o FEATURES diz "English only" |
| Lixeira | Apaga direto | Não verifiquei | `.trash/` no cofre (no Windows também cai na `.trash/`); gravação atômica; detecção de conflito ao salvar |

### Tabela com o nosso Rustidian

| Critério do Bruno | Rustidian (nosso fork) | Hu-Tao128 | opensidian | RustSidian |
|---|---|---|---|---|
| Windows | Sim | Sim | **Não** | Sim |
| Android | **Não ainda** (R7: protótipos prontos, falta o teste do dono) | Não | Não | Não (só no roteiro) |
| Cofre Markdown compatível | Sim | Parcial (sem `.obsidian`) | Sim, o mais fiel | Sim |
| Sync cifrado no próprio servidor | **Sim** (`nota-cripto`, `nota-sync`, `nota-servidor`) | Não | Não | Não (só no roteiro) |
| Guarda as duas versões, lixeira, trava contra exclusão em massa | **Sim, com teste de propriedade** | Não | Não | Só lixeira e aviso de conflito ao salvar; sem sync |
| pt-BR (corretor) | **Sim** | Não | Não vi | Não (só inglês) |
| Desempenho | Bom (egui); medição `RUSTIDIAN_PERF` | Muito leve (feito para Core 2 Duo) | WebView: mais pesado | Declara 34 MB de RAM com 10 mil notas e abrir 5 mil notas em menos de 100 ms (medido por eles em WSL2, não por nós) |
| Maturidade / risco | Original de 1 autor, mas as crates de sync são nossas | Parado há 4 semanas, 2 estrelas: risco alto | Ativo, GPL, feito quase todo por IA, só Linux | Muito novo (6 dias), ativo, 1 autor; muita coisa, pouca vivência |

### Veredito

**O melhor dos três é o RustSidian**, por larga margem: é o único que roda no Windows e tem editor completo (tabelas, multi-cursor, Vim, abas divididas, canvas, grafo, plugins) e testes sérios, e a licença (MIT ou Apache) deixa copiar código. O opensidian é o clone mais fiel do Obsidian, mas é GPL e só Linux. O Hu-Tao128 está parado e é o mais simples dos três.

**Nenhum dos três supera o nosso no que o Bruno pediu como obrigatório:** nenhum tem Android, nenhum tem sync (o nosso tem, cifrado, com as duas versões, lixeira e trava de exclusão em massa), nenhum tem corretor em português. Onde o RustSidian supera o nosso hoje: **recursos do editor** (edição de tabela, multi-cursor, desfazer em árvore, Vim, abas divididas, `.canvas`, fórmulas e Mermaid) e **medição de desempenho nos testes**. Onde o opensidian supera: **assinatura verificada das atualizações** e **fidelidade com o Obsidian** (temas, plugins, favoritos). Nada disso muda a decisão: **manter o fork**. Gatilho para reabrir: o RustSidian entregar sync cifrado e Android (reavaliar em fevereiro/2027, junto com o SoloMD).

Cuidado com o RustSidian: o editor dele é próprio (não é o `TextEdit` do egui), então o código de `editor-core` não encaixa direto no nosso; aproveita-se a lógica pura (tabela, Vim) e o desenho dos testes, não a tela. Também é novo demais para valer como base.

### O que ainda vale trazer (por valor)

**Pode copiar código (MIT ou Apache; crédito em `THIRD_PARTY_NOTICES.txt`):**

| # | Ideia | De onde | Licença |
|---|---|---|---|
| 1 | **Edição de tabela** (Tab entre células, alinhar colunas, inserir linha/coluna) | RustSidian, [`crates/editor-core/src/table.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/editor-core/src/table.rs) | MIT ou Apache-2.0 |
| 2 | **Abas divididas** (painéis lado a lado, mesmo arquivo em dois painéis, layout restaurado) | RustSidian, [`crates/app/src/split.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/app/src/split.rs) e [`panes.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/app/src/panes.rs) | MIT ou Apache-2.0 |
| 3 | **Desfazer em árvore e multi-cursor** (lógica de buffer pura, com teste de propriedade) | RustSidian, [`buffer.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/editor-core/src/buffer.rs), [`editor.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/editor-core/src/editor.rs) e [`tests/prop.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/editor-core/tests/prop.rs) | MIT ou Apache-2.0 |
| 4 | **Modo Vim** (opcional, desligado por padrão) | RustSidian, [`crates/editor-core/src/vim.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/editor-core/src/vim.rs) | MIT ou Apache-2.0 |
| 5 | **Leitor/editor de `.canvas`** (JSON Canvas sem perder campos que não entende) | RustSidian, [`crates/canvas-core/src/lib.rs`](https://github.com/siddarth1872004/RustSidian/blob/main/crates/canvas-core/src/lib.rs) | MIT ou Apache-2.0 |
| 6 | **Grafo só se ligar na compilação** (a build padrão nem carrega a biblioteca de grafo) | Hu-Tao128, [`rustidian-core/src/graph.rs`](https://github.com/Hu-Tao128/rustidian/blob/main/rustidian-core/src/graph.rs) e o `feature = "graph"` do [`Cargo.toml`](https://github.com/Hu-Tao128/rustidian/blob/main/rustidian-ui/Cargo.toml) | MIT |
| 7 | **Atualização com conferência de hash e troca do executável em uso** (`self-replace`); serve de base, mas falta a assinatura (item 8) | Hu-Tao128, [`rustidian-ui/src/update.rs`](https://github.com/Hu-Tao128/rustidian/blob/main/rustidian-ui/src/update.rs) | MIT |

**Só ideia (GPL-3.0: não copiar código, para o nosso fork continuar MIT):**

| # | Ideia | De onde |
|---|---|---|
| 8 | **Assinatura das atualizações**: baixar `SHA256SUMS` e `SHA256SUMS.asc`, conferir contra uma chave embutida no programa (impressão digital de 40 dígitos fixada no código) e exigir que cada linha seja da mesma versão, para um arquivo antigo assinado não passar por novo | opensidian, [`src-tauri/src/updates.rs`](https://github.com/jpzk/opensidian/blob/main/src-tauri/src/updates.rs) (o comentário no topo descreve o desenho inteiro) |
| 9 | **Favoritos do Obsidian** (a pasta de teste `bmcompat` sugere `bookmarks.json` já testado), **PDF dentro da nota** e **`.canvas` pelo lado do arquivo** | opensidian: [`pdf.rs`](https://github.com/jpzk/opensidian/blob/main/src-tauri/src/pdf.rs), [`canvasfs.rs`](https://github.com/jpzk/opensidian/blob/main/src-tauri/src/canvasfs.rs), [`tests/fixtures/bmcompat`](https://github.com/jpzk/opensidian/tree/main/src-tauri/tests/fixtures/bmcompat) (li só os nomes, não o conteúdo) |
| 10 | **Travar o cofre contra dois programas abertos ao mesmo tempo** | opensidian, [`src-tauri/src/vaultlock.rs`](https://github.com/jpzk/opensidian/blob/main/src-tauri/src/vaultlock.rs) (só o nome; não li) |

Prioridade sugerida: (1) tabela, (2) `.canvas`, (3) abas divididas, (4) assinatura das atualizações (item 8, com o item 7 como ponto de partida), (5) multi-cursor e desfazer em árvore. O Vim fica por último (uso do Bruno incerto). O corretor pt-BR já temos e nenhum deles tem.

### O que não foi conferido

- Nada foi compilado nem rodado; números de desempenho e de testes do RustSidian são declarações do README/FEATURES deles, medidas em WSL2.
- Não li o código de `table.rs`, `vim.rs`, `canvas-core` e `split.rs` para ver se separam mesmo a lógica da tela; o encaixe no egui é a parte de risco.
- Do opensidian, `pdf.rs`, `canvasfs.rs` e `vaultlock.rs` só têm o nome lido; "sem sync" e "sem corretor" vêm do README, não de busca no código.
- "Pelo menos 100 commits" do opensidian na semana: a consulta da API corta em 100.
- Estrelas e commits mudam por dia; o RustSidian tem 6 dias de vida e pode mudar bastante até fevereiro.
