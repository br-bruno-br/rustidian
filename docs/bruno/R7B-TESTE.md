# R7b-1: roteiro de teste do app Android

O primeiro Rustidian de verdade para o celular: um caderno de notas. **Ainda não sincroniza** (isso vem na R7b-3) e **ainda não mostra Markdown bonito** (R7b-2): você escreve e lê o texto puro. Serve para dizer se o dia a dia de anotar funciona.

Arquivo: `rustidian-android-r7b1.apk`.

## 1. Instalar

1. Passe o `.apk` para o celular (cabo, Drive ou Tailscale) e abra pelo app **Arquivos**.
2. Se o Android reclamar de "apps desconhecidos", toque em **Configurações** e ligue **Permitir desta fonte**. Volte e toque em **Instalar**.
3. Se aparecer "Play Protect: app não verificado", toque em **Instalar mesmo assim** (o app está assinado com chave de teste).
4. O app se chama **Rustidian** (pacote `dev.rustidian.app`). Ele **não** substitui os apps de teste da R7a; pode ficar com os dois.

## 2. Onde ficam as notas

O app guarda tudo numa pasta de arquivos `.md` comuns:

`Android/data/dev.rustidian.app/files/Cofre`

Na tela inicial, bem embaixo da lista, o app mostra o caminho certo. Não pede permissão de acesso a todos os arquivos.

- **Pelo celular:** o app **Arquivos do Google** esconde a pasta `Android/data` no Android 11 ou mais novo. Use o **Meus Arquivos** da Samsung, o Solid Explorer ou parecido. Se nenhum mostrar, não é defeito do app: anote o aparelho e o Android.
- **Pelo cabo no PC:** o aparelho aparece como disco; abra `Android/data/dev.rustidian.app/files/Cofre`.
- Se aparecer uma **faixa amarela** na tela inicial dizendo que as notas estão na memória interna, o Android não deixou criar a pasta compartilhada. Me avise (o app funciona, mas o gerenciador de arquivos não vai enxergar as notas).

## 3. Testar

Use o **Gboard** com o autocorretor ligado. Anote o que não ficar certo.

1. **Abrir.** O app abre numa tela "Rustidian", vazia, com a barra de busca, os botões **Nova nota** e **Nova pasta**, e o caminho do cofre.
2. **Nota nova.** Toque em **Nova nota**. Abre uma nota chamada "Sem título". O teclado sobe? Digite: `ação, pão e café com açúcar` e um emoji. Os acentos saem certos? O autocorretor e as sugestões funcionam?
3. **Salvar sozinho.** Embaixo da tela aparece **Salvando…** enquanto você digita e **Salvo** cerca de 1 segundo depois que para.
4. **Voltar.** Aperte o **Voltar do Android** (gesto ou botão). Tem que voltar para a lista **sem fechar o app**. A nota está na lista. Na lista, o Voltar do Android fecha o app.
5. **Reabrir.** Feche o app de verdade (recentes, deslize) e abra de novo. A nota continua lá com o texto?
6. **Renomear.** Abra a nota e toque no **título** (em cima). Digite outro nome e toque em **Renomear**. O título muda. Crie outra nota, renomeie para o mesmo nome: ela vira `nome 2` (nunca sobrescreve).
7. **Pastas.** Na lista, **Nova pasta**, escreva um nome e **Criar**. Toque na pasta: abre vazia, com a seta ← no topo. Crie uma nota lá dentro, volte (← ou Voltar do Android) e confira.
8. **Busca.** Crie duas ou três notas com palavras diferentes. Na barra de busca, escreva parte de um **nome** e depois uma palavra que só está **dentro** de uma nota. Os dois acham? Quem casa no nome vem primeiro; quem casa no texto mostra a linha onde a palavra aparece.
9. **Lixeira.** Toque no ícone da lixeira ao lado de uma nota. Aparece a pergunta; confirme. A nota some da lista, mas **não foi apagada**: no gerenciador de arquivos ela está em `Cofre/.trash/`. Faça o mesmo dentro de uma nota (lixeira no topo) e com uma pasta.
10. **Arquivos de fora.** Pelo gerenciador de arquivos (ou pelo PC), copie um `.md` seu para dentro do `Cofre`. Volte ao app e abra a lista de novo: ele aparece? Abra, edite, e confira no PC que o arquivo mudou.
11. **Segundo plano.** Digite uma frase e **aperte o botão Início logo em seguida** (sem esperar o "Salvo"). Abra o app de novo: a frase está lá?
12. **Tema.** Troque o tema do celular entre claro e escuro. O app acompanha? Dá para ler tudo nos dois?
13. **Teclado e tela.** Cole ou digite texto até passar de uma tela. Toque na última linha: ela continua visível acima do teclado? Gire o celular: nada se perde?
14. **Sensação.** Escreva um parágrafo rápido. Atraso? Travada? Rolou bem com o dedo?

## 4. O que me contar

- Cada item que não deu certo (número e o que aconteceu).
- Modelo do celular e versão do Android.
- Se o gerenciador de arquivos que você usa mostra a pasta `Cofre`.
- O que **faltou** para usar de verdade no dia a dia (isso decide as próximas fatias).

## 5. Não faz parte desta versão

Sincronização com o servidor, visualização do Markdown, abrir links entre notas, imagens, anexos, restaurar da lixeira (por enquanto: copie o arquivo de volta de `.trash`) e escolher outra pasta para o cofre.
