<div align="center">

# Rustidian

<table>
  <tr>
    <td align="center"><img src="docs/brand/themes/signature.png" width="112" height="112" alt="Logo Rustidian, tema Signature"><br><sub>Signature</sub></td>
    <td align="center"><img src="docs/brand/rustidian.png" width="112" height="112" alt="Logo Rustidian, verde-azulado"><br><sub>Teal</sub></td>
    <td align="center"><img src="docs/brand/themes/love-space.png" width="112" height="112" alt="Logo Rustidian, tema Love Space"><br><sub>Love Space</sub></td>
    <td align="center"><img src="docs/brand/themes/elpis-gold.png" width="112" height="112" alt="Logo Rustidian, tema Elpis Gold"><br><sub>Elpis Gold</sub></td>
  </tr>
</table>

**Um caderno de notas em Markdown, nativo em Rust. Seus arquivos, suas pastas, sem conta.**

Português · [English](README.en.md)

</div>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/rustidian-light.png">
  <img src="docs/images/rustidian.png" alt="Rustidian mostrando o cofre de demonstração">
</picture>

## O que é

O Rustidian guarda as notas em pastas comuns de arquivos Markdown. Não pede conta e abre cofres do Obsidian direto, sem importar nada. Tudo é nativo em Rust, da interface ao editor.

Este repositório é um fork do [Rustidian de Masih Moafi](https://github.com/MasihMoafi/rustidian), feito para o dono largar o Obsidian e usar o mesmo editor no Windows, no Android e no Linux. Independente, sem ligação com o Obsidian.

## O que já funciona

**No computador (Windows e Linux)**
- App para **Windows**: seletor de arquivos nativo, tema e fontes do sistema.
- **Interface em português do Brasil** e **corretor ortográfico pt-BR** (dicionário baixado pelo botão "Baixar dicionário pt-BR" e conferido por sha256).
- **Tela dividida**: dois painéis, lado a lado ou um sobre o outro, cada um com as suas abas. A mesma nota nos dois painéis é uma só.
- **Tabelas**: inserir, colar da planilha, converter seleção e alinhar.
- **Menções sem link**: o painel de backlinks lista as notas que citam o título da nota aberta, e o botão **Link** transforma a menção em `[[link]]`.
- **Cofre grande e vigia confiável**: índice incremental, abre cofres de dezenas de milhares de notas e não perde nota por causa de sync ou de editor externo.
- **Atualizações assinadas**: o app só instala o que bate com a chave pública embutida. A chave pública do dono já está no app; o primeiro release assinado sai com o instalador do Windows.

**Quadro estilo Heptabase** (arquivos `.canvas`, o mesmo formato aberto do Obsidian)
- Cada nota vira um **cartão vivo**: mostra o texto formatado e edita a nota no lugar.
- **Seção como cartão** (só um trecho da nota) e **quadro dentro de quadro** (miniatura do quadro menor).
- **Organizar**: alinhar, distribuir, arrumar em grade e **mapa mental** a partir de um cartão.
- **Etiquetas e filtro** por cor e por etiqueta.

**Histórico das notas**
- Versões de cada nota, guardadas pela diferença (ocupa pouco espaço), com restaurar e ver a diferença.
- **Linha do tempo** com barra deslizante, **versões com nome** e **apagar do histórico**.
- **Desfazer que continua** depois de fechar a nota ou o app, com galhos; **achar o que apaguei** e **histórico de um trecho**.
- Retenção configurável nas configurações.

**Sincronização cifrada**
- As peças existem e têm testes: `nota-cripto` (cifra de ponta a ponta), `nota-sync` e `nota-servidor` (o servidor guarda só conteúdo cifrado). No desktop há tela de configuração, ícone de estado e ciclo em segundo plano. Na dúvida, guarda as duas versões.
- Ainda não está em uso diário: o servidor próprio e o backup ficam para a fila abaixo.

**Android (fase inicial)**
- App em Dioxus com lista de notas, busca, editor com salvamento automático e lixeira. Ainda mostra o texto cru e ainda não sincroniza.

## Seus dados estão seguros

Garantias que existem no código, com a contagem de testes marcados com `#[test]` na área (contagem por `grep`, sem compilar; não é cobertura).

| Garantia | Como | Testes |
| --- | --- | --- |
| Nota nunca é apagada de vez | O que some vai para `.trash/` do cofre (é um `rename`, nunca `rm`), e o app pede confirmação antes de mandar a nota para a lixeira (dá para desligar o aviso) | `crates/nota-sync` 62; telas em `src/app/gui_tests` 270 |
| Conflito não perde nada | Se a nota mudou nos dois lados, uma versão vira o arquivo principal e a outra é salva como `nome (conflito aparelho data).md` | `crates/nota-sync` 62 |
| Servidor não lê as notas | Cifra de ponta a ponta: o servidor guarda só conteúdo cifrado. Cada aparelho tem a cópia completa e funciona sem rede | `crates/nota-cripto` 19, `crates/nota-servidor` 20 |
| Gravação sem arquivo pela metade | Escrita atômica: arquivo temporário na mesma pasta, `sync_all` e `rename` por cima | `crates/nota-sync` 62 |
| Nota que pode ser estragada não abre | Nota grande ou que não está em UTF-8 não é aberta, para não estragar o arquivo. Limites: 8 MB no índice do desktop (`.md` maior fica fora do índice, com aviso) e 2 MB no Android | `src/app/gui_tests` 270; `android/` 51 |
| Vigia não perde nota | Uma nota que a varredura não achou só sai do índice depois de conferida duas vezes | `src/app/gui_tests` 270 |
| Histórico das notas | Versões automáticas por diferença, retenção configurável (padrão: todas por 1 dia), restaurar, versões com nome (nunca saem) e "Apagar do histórico" tira um trecho de todas as versões, sem desfazer | `crates/nota-historico` 65 |
| Atualização é a que o dono assinou | O app só instala o que bate com a chave pública embutida (ed25519 e SHA256SUMS). A chave do dono já foi gerada e a pública está no repositório | `src/atualizacao.rs` 13 |
| Desfazer que continua depois de fechar | A árvore do desfazer é gravada no histórico e volta ao reabrir a nota, com galhos; se o arquivo mudou por fora, ela não é aplicada. "Achar o que apaguei" lista os trechos que sumiram e deixa reinserir | `crates/nota-historico` e telas `historico_h3h4` |
| Sync cifrado no Android e backup no Google Drive | **Em andamento**, veja o roadmap | ainda não |

## Roadmap

A fila anda **uma frente por vez**: compilar em paralelo trava o PC do dono (2 núcleos), e cada fatia só começa depois que a anterior foi verificada e juntada. Sem datas.

| Agora | Próximo | Depois |
| --- | --- | --- |
| **Markdown formatado no Android** (ler a nota como no PC, com tabelas e tarefas) | | **Release para Windows** com instalador (em breve) |
| | **Sync cifrado no Android** (endereço, token e frase, estado e conflitos) | **Linux portátil** com pacote assinado pelo dono |
| | **Servidor de sync próprio** (no servidor de casa, só pela Tailscale) **e backup cifrado no Google Drive** | Terminar de trocar os scripts Python por Rust (`cargo xtask` e testes `egui_kittest`) |

Estado de cada passo: [docs/bruno/FILA-ANDROID-SYNC.md](docs/bruno/FILA-ANDROID-SYNC.md). Plano completo, com as fatias e o que depende do dono: [docs/bruno/PLANO.md](docs/bruno/PLANO.md). Desenho do histórico: [docs/bruno/HISTORICO-NOTAS.md](docs/bruno/HISTORICO-NOTAS.md).

## Como compilar

Este fork ainda não tem release próprio. Release para Windows: em breve. Por enquanto, compile.

**Windows** (Rust com o alvo MSVC):

```powershell
cargo build --release --bin rustidian
.\target\release\rustidian.exe
```

O executável sai em `target\release\rustidian.exe`. Em **Arquivo → Abrir pasta...** escolha uma cópia do seu cofre.

**Linux** (x86_64, glibc 2.39 ou mais nova, como o Ubuntu 24.04; desktop X11 ou Wayland):

```sh
cargo build --release --bin rustidian
./target/release/rustidian "/caminho/do/cofre"
```

Os diálogos de abrir usam o `zenity`; instale pelo gerenciador de pacotes. Os pacotes `.deb`, AppImage e snap citados na documentação do original são do projeto dele, não deste fork.

Atalhos: Ctrl+P paleta de comandos, Ctrl+O troca rápida de nota, Ctrl+N nova nota, Ctrl+E alterna leitura e edição, Ctrl+Shift+F busca no cofre, Ctrl+S salva. O app salva sozinho depois de 600 ms parado. Plugins da comunidade do Obsidian não rodam.

## Medir lentidão

Com `RUSTIDIAN_PERF=<arquivo>`, o app acrescenta ao arquivo uma linha JSON por ação (`abrir_cofre`, `salvar`, `buscar`, `trocar_aba`, `quadro_lento` e outras), com o tempo em ms e o tamanho do cofre. Sem a variável, não abre arquivo nem lê relógio. No PowerShell:

```powershell
$env:RUSTIDIAN_PERF = "$env:TEMP\rustidian-perf.jsonl"; .\target\release\rustidian.exe
```

Sobre velocidade: o autor do original mediu 0,61 s contra 3,77 s do Obsidian para abrir uma nota, no Linux, numa build antiga e num único ambiente ([teste](docs/performance/startup-2026-10-22/README.md)). Não é medida deste fork nem do Windows.

## Créditos e licença

- Original: [Rustidian, de Masih Moafi](https://github.com/MasihMoafi/rustidian).
- A ideia das menções sem link, da tela dividida e dos quadros vem do [RustSidian](https://github.com/siddarth1872004/RustSidian) (MIT); o código é nosso.
- Corretor: [spellbook](https://github.com/helix-editor/spellbook) com o dicionário VERO (LibreOffice).
- Bibliotecas de terceiros: [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
- Licença: [MIT](LICENSE).

## Contribuir com o original

Correções que servem a todos saem em branches `upstream/<assunto>`, cada uma com um commit limpo em inglês, para virar Pull Request no original; o que é só do fork (Windows, sync, Android, servidor do dono) fica no `main`. Veja [docs/bruno/PLANO.md](docs/bruno/PLANO.md) e o [CONTRIBUTING.md](CONTRIBUTING.md).
