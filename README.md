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
- **Atualizações assinadas**: o app só instala o que bate com a chave pública embutida. A chave do dono ainda não foi gerada.

**Quadro estilo Heptabase** (arquivos `.canvas`, o mesmo formato aberto do Obsidian)
- Cada nota vira um **cartão vivo**: mostra o texto formatado e edita a nota no lugar.
- **Seção como cartão** (só um trecho da nota) e **quadro dentro de quadro** (miniatura do quadro menor).
- **Organizar**: alinhar, distribuir, arrumar em grade e **mapa mental** a partir de um cartão.
- **Etiquetas e filtro** por cor e por etiqueta.

**Histórico das notas**
- Versões de cada nota, guardadas pela diferença (ocupa pouco espaço), com restaurar e ver a diferença.
- **Linha do tempo** com barra deslizante, **versões com nome** e **apagar do histórico**.
- Retenção configurável nas configurações.

**Sincronização cifrada**
- As peças existem e têm testes: `nota-cripto` (cifra de ponta a ponta), `nota-sync` e `nota-servidor` (o servidor guarda só conteúdo cifrado). No desktop há tela de configuração, ícone de estado e ciclo em segundo plano. Na dúvida, guarda as duas versões.
- Ainda não está em uso diário: o servidor próprio e o backup ficam para a fila abaixo.

**Android (fase inicial)**
- App em Dioxus com lista de notas, busca, editor com salvamento automático e lixeira. Ainda mostra o texto cru e ainda não sincroniza.

## Roadmap

A fila anda **uma frente por vez**: compilar em paralelo trava o PC do dono (2 núcleos), e cada fatia só começa depois que a anterior foi verificada e juntada. Sem datas.

| Agora | Próximo | Depois |
| --- | --- | --- |
| **Histórico:** desfazer que continua depois de fechar o app e achar o que eu apaguei | **Markdown formatado no Android** (ler a nota como no PC, com tabelas e tarefas) | **Release para Windows** com instalador (em breve) |
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
