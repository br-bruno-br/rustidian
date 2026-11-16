# R7a: roteiro de teste no celular

Aplicativos de teste, feitos só para descobrir **qual deles digita melhor com o Gboard**. Eles não mexem nas suas notas: cada um grava um único arquivo `nota.md` dentro da própria pasta privada.

- `rustidian-proto-egui-v2.apk` (o mesmo desenho do Rustidian do PC; **v2** corrige os três problemas achados no primeiro teste: Enter, arrastar para rolar e emoji). O `rustidian-proto-egui.apk` antigo não precisa mais ser testado: a v2 instala por cima dele.
- `rustidian-proto-dioxus.apk` (uma página web dentro do app)

Os dois têm a mesma tela: um botão **Salvar**, uma linha de status, um campo grande de texto e uma linha pequena embaixo com o número de caracteres.

## 1. Instalar

1. Passe os dois arquivos `.apk` para o celular (cabo, Drive ou Tailscale).
2. Abra o arquivo pelo app **Arquivos** (ou pelo navegador, se baixou por ele).
3. O Android vai avisar "por segurança, o celular não pode instalar apps desconhecidos desta fonte". Toque em **Configurações** e ligue **Permitir desta fonte** para o app que você usou (Arquivos ou Chrome). Volte e toque em **Instalar**.
4. Se aparecer "Play Protect: app não verificado", toque em **Instalar mesmo assim**. É normal: o app é de teste e está assinado com chave de teste.
5. Repita para o segundo `.apk`. Os dois podem ficar instalados ao mesmo tempo ("Proto egui" e "Proto Dioxus").

Quando terminar os testes, você pode desligar de novo "instalar apps desconhecidos" e desinstalar os dois (segure o ícone, **Desinstalar**).

## 2. Testar (faça os mesmos passos nos dois apps)

Use o **Gboard** com o **autocorretor ligado** (Configurações do Gboard, Correção de texto, Correção automática). Anote o que acontecer em cada passo na tabela da seção 3.

1. **Abrir.** Abra o app. Toque no campo de texto. O teclado deve subir sozinho. Anote se não subiu.
2. **Letras simples.** Digite `ola mundo`. Aparecem as letras certas, sem repetir nem pular?
3. **Acentos.** Digite, com o teclado normal e sem copiar de lugar nenhum: `ação, pão, café com açúcar`.
   - Os acentos aparecem como você digitou?
   - Para o `ç`: segure o `c` e escolha o `ç`. Funciona?
4. **Autocorretor e sugestões.** Digite `voce` e depois um espaço. O Gboard troca por `você`? Digite `nao ` e veja se vira `não `. As sugestões aparecem na faixa acima do teclado e, ao tocar numa, a palavra é trocada de verdade no texto?
5. **Apagar.** Apague uma letra com a tecla de apagar. Segure a tecla para apagar várias. Apague uma palavra inteira que acabou de corrigir. Anote se apagou a mais, a menos, ou nada.
6. **Linha nova.** Toque em Enter. Cria uma linha nova? Digite uma segunda linha. Aperte Enter no meio de uma palavra: quebra ali?
7. **Colocar o cursor.** Toque no meio de uma palavra. O cursor vai para lá? Digite uma letra: ela entra onde está o cursor (e não no fim do texto)?
8. **Selecionar com o dedo.** Segure o dedo parado sobre uma palavra por meio segundo (toque longo) ou toque duas vezes. A palavra fica selecionada? (No **Proto egui v2**, ao segurar parado, arrastar em seguida estica a seleção; arrastar logo de cara rola o texto.) Aparecem as alças para esticar a seleção e o menu Copiar/Colar? Arraste uma alça. Digite por cima: substitui a seleção?
9. **Rolar.** Cole ou digite texto até passar de uma tela cheia (uns 30 linhas curtas). Arraste o dedo para cima e para baixo sobre o texto. Rola, ou em vez de rolar ele seleciona texto? Anote. Toque curto no meio do texto deve só pôr o cursor, sem selecionar nada.
10. **Teclado cobrindo o texto.** Role para o fim do texto e toque na última linha. A linha em que você está digitando fica visível acima do teclado, ou fica escondida atrás dele? Digite mais linhas e veja se continua visível.
11. **Girar a tela.** Gire o celular para deitado e depois em pé. O texto continua lá? O app não reinicia e não perde o que digitou? O teclado continua funcionando?
12. **Salvar.** Toque em **Salvar**. A linha de status deve dizer "Salvo: N bytes ... em /data/...". Feche o app de verdade (recentes, deslize para cima) e abra de novo: o texto voltou? A linha de status deve dizer "Lido de ...".
13. **Emoji.** Abra os emojis do Gboard e toque em alguns (por exemplo 😀 👍 ❤️). Eles aparecem no texto? Colorido ou só um risquinho/quadrado? Apague o emoji com a tecla de apagar: some inteiro? No egui v2 a linha pequena de baixo diz "emoji colorido: ..." com o nome da fonte que achou; anote o que está escrito.
14. **Sensação.** Digite um parágrafo de umas 5 linhas rápido, como escreveria uma nota. Tem atraso entre tocar e a letra aparecer? Algum travamento?

Só no **Proto egui**: na linha pequena de baixo aparece "eventos do teclado: N | último: ... | tecla: ...". Anote se o número sobe quando você digita e o que aparece em "tecla" depois de apertar Enter ou apagar. Se ficar em 0 mesmo digitando e aparecendo letras, o teclado está entrando por outro caminho; se ficar em 0 e **nada** aparece, o app de egui não recebeu o texto do Gboard.

## 3. O que anotar

Copie a tabela e preencha com **OK**, **falhou** ou **mais ou menos**, mais uma frase do que viu.

| Passo | Proto egui | Proto Dioxus |
|---|---|---|
| 1 Teclado sobe ao tocar | | |
| 2 Letras simples | | |
| 3 Acentos (ação, pão, café com açúcar) | | |
| 4 Autocorretor e sugestões | | |
| 5 Apagar | | |
| 6 Enter | | |
| 7 Cursor no meio do texto | | |
| 8 Selecionar com o dedo, alças, copiar/colar | | |
| 9 Rolar | | |
| 10 Teclado não cobre o texto | | |
| 11 Girar a tela | | |
| 12 Salvar e reabrir | | |
| 13 Emoji | | |
| 14 Sensação (atraso, travada) | | |

Informe também: modelo do celular, versão do Android e se o teclado é mesmo o Gboard.

Se um app **fechar sozinho** ao abrir ou no meio do teste, anote o passo em que aconteceu. Isso conta como falha, e é uma informação importante.

## 4. Como decidir

O que mais pesa para escrever notas todo dia, nesta ordem: acentos e autocorretor (3 e 4), apagar (5), cursor e seleção (7 e 8), rolar e teclado cobrindo (9 e 10). Quem falhar em 3, 4 ou 5 está fora, mesmo que o resto funcione.
