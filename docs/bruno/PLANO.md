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

### Backup
- No re-servidor, o **rustic** (Rust, compatível com restic) guarda todo dia a pasta de dados do `nota-servidor` no Google Drive. Essa pasta já está cifrada, e o rustic cifra de novo.
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
| R7 | Android: protótipo comparando egui (android-activity) e Dioxus para digitar acentos pelo Gboard, selecionar e rolar; depois o app com o mesmo miolo | APK instalado no celular do dono, sincronizando |

R2 e R3 tocam arquivos diferentes e podem correr em paralelo.

## O que depende do dono
- **R6:** login no Google para o rustic, e criar e guardar as duas senhas.
- **R7:** ok para instalar o Android SDK e o NDK (~5–8 GB) no PC; instalar o APK no celular.
- Escolher a frase-senha do cofre (só ele digita).
