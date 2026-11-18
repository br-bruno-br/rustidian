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
