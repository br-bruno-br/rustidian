# Fila: Markdown no Android e sync (pedido do dono em 20/11/2026)

Uma frente por vez: compilar Rust em paralelo trava o PC de 2 núcleos. Cada fatia começa só depois que a anterior foi verificada e juntada na main. Este arquivo é o estado da fila: quem retomar lê aqui primeiro.

| ordem | fatia | quem | estado |
|---|---|---|---|
| 0 | Histórico H3/H4 (desktop) | s-h | juntado em 20/11 (353 testes do app, 224 da lib, 159 dos crates) |
| 1 | **R7b-2** Markdown no Android | s-h | pronta na branch `r7b2-markdown` (20/11), esperando verificação, juntada e o teste do APK pelo dono |
| 2 | **R7b-3** Sync no Android (contra servidor local nos testes) | s-h | esperando |
| 3 | **R6** Servidor no re-servidor + backup rustic → Drive | eu, com o dono | esperando o dono |

Por que essa ordem: o código da R7b-3 é testado contra um `nota-servidor` local, então não depende da R6. A R6 mexe numa máquina de verdade e precisa do dono (login do Google, senhas), por isso vai por último e com ok.

## Regras que valem para as três

- **Ambiente:** `CARGO_TARGET_DIR` próprio da fatia em `C:/dev/_wt/target-<fatia>`, `CARGO_BUILD_JOBS=1`, compilação e testes em primeiro plano, nunca nada em segundo plano no fim (sessão sem tela mata o que fica rodando).
- **Fronteira:** `android/` tem workspace próprio. A R7b-2 e a R7b-3 não mexem em `crates/` nem em `src/` do desktop. Se precisarem mudar `crates/nota-sync`, param e me avisam.
- **Gate do Android** (em `android/`): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` e `cargo test`.
- **Prints:** pelo `dioxus-ssr` e pelo Chrome headless em 390x844, claro e escuro (ver "Verificação e build" na seção R7b do PLANO). Nunca abrir janela nem clicar na tela do dono.
- **APK:** só no fim de cada fatia de Android, com `android/construir.sh`. O primeiro build levou cerca de 45 min.
- **Commits:** pequenos, identidade `br-bruno-br`, sem push. O push é meu, depois de juntar.
- **Lixo:** nunca apagar pasta em massa nem criar junção. As pastas `target-*` antigas o dono apaga.

## 1. R7b-2: Markdown no Android

**Por que:** hoje o celular só mostra o texto cru. O dono quer ler as notas como no PC.

**O que fazer:**
- Rota nova `Leitura` em `rota.rs`, com um braço no `match` de `ui/mod.rs` e uma tela em `ui/leitura.rs`.
- Botão no topo da nota para alternar entre Editar e Ler. Abrir nota continua no editor; guardar a última escolha por nota fica para depois.

**Como renderizar:**
- `pulldown-cmark` na mesma versão do desktop (0.13.4), com tabelas, tarefas, tachado e notas de rodapé.
- Num módulo sem Dioxus, `logica/markdown.rs`, testável no Windows, que devolve um HTML **seguro**.

**Armadilhas:**
- **HTML cru da nota nunca é executado.** As notas chegam pelo sync de outro aparelho, e a WebView tem ponte para o Rust. Os eventos `Html` e `InlineHtml` viram texto escapado, e links `javascript:` e `data:` são descartados.
- **Links:**
  - `[[nota]]` e `[texto](Nota.md)` abrem a nota no app.
  - `http(s)` vai para o navegador do sistema, ou fica só como texto se não houver jeito simples sem JNI.
  - A WebView nunca navega para fora.
- **Imagens:** só as que estão dentro do cofre (`caminho_dentro`). As de fora viram texto "imagem: nome".
- **Caixa de tarefa:** tocar marca ou desmarca e grava no arquivo pelo mesmo `Rascunho` e `escrever_atomico`, trocando só o `[ ]` daquela linha. Se o arquivo mudou por fora desde que a tela abriu, não grava e avisa.
- **Nota grande:** respeita os limites da R7b-1 (2 MB e só UTF-8).

**Testes:**
- Unidade em `logica/markdown.rs`: escapes, links, `[[ ]]`, tarefas e tabela.
- Prints da tela Leitura com uma nota de exemplo que tenha título, lista, tabela, tarefa, código e um `<script>` que **não** pode rodar.

**Pronto quando:** o gate do Android passa, os prints foram olhados, o APK novo foi gerado e o dono o recebeu para testar.

## 2. R7b-3: Sync no Android

**Por que:** sem sync, o celular é um cofre separado do PC.

**O que fazer:**
- Tela `Sincronizar` com endereço, token e frase-senha.
  - A frase-senha não fica em arquivo; guarde só o que o `nota-sync` já guarda no desktop, seguindo o mesmo desenho.
  - A tela mostra o estado: última sincronização, erro e conflitos.
- `Servicos` ganha o sincronizador (`nota-sync` como biblioteca).
- O ciclo roda ao abrir o app, ao salvar e ao voltar para o app.
- Renomear e o sync gravando o mesmo nome ficam serializados (pendência da R7b-1).

**Armadilhas:**
- **Conflito:** guarda as duas cópias, nunca apaga.
- **Rede:** ureq sem TLS. Só funciona pelo Tailscale, e a tela tem de dizer isso.
- **Ciclo:** não pode travar a tela, então roda numa thread.

**Testes:** integração com `nota-servidor` local na porta 0 e dois cofres temporários, cobrindo três casos:
- os dois cofres convergem;
- um conflito gera as duas cópias;
- uma nota apagada vai para a lixeira nos dois lados.

**Pronto quando:** o gate do Android passa, esses testes de integração passam, os prints foram olhados e o APK foi gerado.

## 3. R6: servidor no re-servidor e backup

Fica comigo e com o dono, não com um executor. Mexe numa máquina de verdade, por isso cada passo de escrita lá é feito com ok.

**O que fazer:**
- Instalar o serviço systemd do `nota-servidor`, escutando só no IP do Tailscale.
- Backup rustic → Google Drive pelo rclone.
  - Usar o client_id próprio do Google, que ainda está pela metade.
  - O login no Google e as duas senhas (a do rustic e a do rclone) são digitados pelo dono.
- Teste de restauração.

**Pronto quando:** o serviço volta sozinho depois de reiniciar o re-servidor, e o backup e a restauração foram testados.

## Pendências conhecidas

- Teste `navegacao::copiar_pasta_leva_notas_binarios_ocultos_e_vazias_e_so_salva_o_rascunho_da_origem` é instável sob carga: o salvamento automático dos rascunhos digitados no teste roda `after_note_saved`, que limpa `app.error` antes da conferência do aviso "só está disponível no Linux". Passou na rodada final; conserto = o teste esperar os salvamentos (`settle_saves`) antes de copiar a pasta.
