# R7b-3: roteiro de teste da sincronização no celular

Arquivo: `rustidian-r7b3-sync.apk` (instala por cima do R7b-2; as notas ficam). O celular passa a sincronizar com o servidor de notas, como o PC.

**Importante: o teste de verdade depende da R6.** O servidor (`nota-servidor`) ainda não está instalado no re-servidor (a R6 faz isso, com o dono). Até lá a sincronização foi testada só contra um servidor local nos testes automáticos, em duas pastas no PC. Este roteiro serve para quando o servidor estiver de pé e a Tailscale ligada no celular. O que dá para testar **antes** da R6 está nos itens 1 e 2.

## O que você precisa ter

- O endereço do servidor na Tailscale, no formato `http://100.x.y.z:8443` (sempre `http://`, nunca `https://`).
- Um **token só para o celular** (no re-servidor: `nota-servidor token novo celular`; o token aparece uma vez).
- A **frase-senha do cofre**: a mesma que você usa no PC. Se este for o primeiro aparelho do servidor, a frase cria o cofre (pelo menos 8 caracteres).
- A Tailscale **ligada no celular** (o app não mostra nem avisa se ela estiver desligada: aparece "Sem conexão").

## Sem servidor ainda (pode testar já)

1. **A tela existe.** Na lista, no topo à direita, há um rótulo "Sincronização desligada". Toque: abre **Sincronizar**, com a linha amarela dizendo que só funciona pela Tailscale ou na rede de casa, e os campos Endereço, Nome deste aparelho, Token e Frase-senha.
2. **Erros claros.** Toque em **Conectar** com tudo vazio: aparece um aviso em português (token vazio ou endereço inválido), sem travar. Digite `https://100.1.2.3:8443`: o aviso diz que tem de começar com `http://`. Digite um endereço qualquer válido (`http://100.1.2.3:8443`), um token qualquer e uma frase: depois de alguns segundos aparece "Não consegui falar com o servidor… confira se a Tailscale está ligada". O app continua normal: abra, edite e salve notas.
3. **Nada mudou no resto.** Editar, Ler, lixeira e busca funcionam como na R7b-2.

## Com o servidor no ar (depois da R6)

4. **Conectar.** Em Sincronizar, preencha endereço, nome (por exemplo "Celular"), token e frase; toque em **Conectar**. O cartão mostra "Sincronizando…" e depois "Sincronizado HH:MM". As notas do PC aparecem na lista. Conte quanto levou.
5. **Frase errada.** Desconecte e conecte de novo digitando uma frase errada: aparece "Frase-senha incorreta…", e o app continua desconectado (nada fica salvo).
6. **Frase a cada abertura.** Feche o app de vez (tire da lista de apps recentes) e abra de novo: o rótulo da lista diz "Digite a frase para sincronizar". Em Sincronizar digite só a frase e toque em **Entrar**. É assim de propósito: a frase não é guardada no celular (veja a decisão no PLANO). Diga se isso incomoda no dia a dia.
7. **Celular → PC.** Crie uma nota no celular, espere uns 5 segundos e veja no PC (ou toque em **Sincronizar agora**). Ela aparece no PC depois do ciclo do PC.
8. **PC → celular.** Edite uma nota no PC. No celular, saia do app e volte (ou toque em **Sincronizar agora**): a nota muda na lista e, se estiver aberta, o texto novo aparece.
9. **Conflito.** Com a Tailscale desligada no celular, edite a nota `X` no celular. Edite a **mesma** nota no PC e sincronize o PC. Ligue a Tailscale no celular e toque em **Sincronizar agora**. O cartão passa a dizer "1 conflito"; em **Sincronizar** aparece a lista com `X (conflito Celular ...)`. Toque nela: abre a cópia (o que você escreveu no celular). A nota `X` tem a versão do PC. **As duas têm de existir.**
10. **Nota aberta enquanto o sync mexe.** Abra uma nota no celular e comece a digitar. No PC, mude a mesma nota e sincronize o celular (botão). Quando você parar de digitar, o app **não pisa** na versão do PC: aparece o aviso "Esta nota mudou em outro aparelho enquanto você escrevia…" com o botão **Abrir a cópia**, e seu texto está na cópia.
11. **Apagar.** Apague uma nota no celular (lixeira). Sincronize os dois lados: ela some da lista nos dois e fica em `.trash` no cofre de cada um.
12. **Sem conexão.** Desligue a Tailscale (ou o Wi-Fi) e escreva uma nota. O rótulo vira "Sem conexão", nada trava, a nota fica salva. Ligue de novo e toque em **Sincronizar agora**: ela sobe.
13. **Desconectar.** Em Sincronizar toque em **Desconectar** e confirme: o rótulo volta a "Sincronização desligada"; as notas continuam no celular e no servidor.

## O que me contar

Número do item e o que apareceu na tela (a mensagem de erro, se houver), o modelo do celular e a versão do Android. Principalmente: (a) se o Voltar do Android e o botão ← continuam voltando direito a partir de Sincronizar; (b) quanto tempo leva a primeira sincronização; (c) se pedir a frase a cada abertura do app é aceitável.
