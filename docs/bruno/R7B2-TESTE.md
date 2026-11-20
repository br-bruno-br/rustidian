# R7b-2: roteiro de teste da leitura formatada

Arquivo: `rustidian-r7b2-leitura.apk` (instala por cima do R7b-1; as notas ficam). Agora cada nota tem o botão **Ler** no topo: ele mostra a nota formatada, como no PC. O texto continua sendo o mesmo arquivo `.md`.

Antes de testar, crie no cofre (pelo app ou pelo gerenciador de arquivos) uma nota `Reunião.md` com isto:

```
# Reunião de sexta

Veja o [[Projeto Alfa]] e o [site](https://exemplo.com).

- [ ] Ligar para o fornecedor
- [x] Mandar a ata

| Item | Qtd |
|:--|--:|
| Cabos | 4 |

<script>alert(1)</script> e <img src=x onerror=alert(1)>
```

e uma nota `Projeto Alfa.md` (qualquer texto).

1. **Ler.** Abra `Reunião` (abre no editor, como sempre) e toque em **Ler**. Aparecem título grande, lista com caixinhas, tabela. O `<script>` e o `<img ...>` aparecem **como texto**, e **nada** abre nem pisca.
2. **Editar.** Toque em **Editar**: volta ao texto cru, no mesmo lugar. O Voltar do Android, de qualquer um dos dois, volta para a lista (não fica preso entre Ler e Editar).
3. **Link de nota.** Toque em **Projeto Alfa**: abre essa nota lida. Voltar (← ou Voltar do Android) retorna para a Reunião.
4. **Link da web.** Toque em **site**: o navegador do celular abre `exemplo.com`. Se não abrir, deve aparecer um aviso amarelo com o endereço (me conte qual dos dois aconteceu e o modelo do celular). O app nunca deve "virar" a página do site.
5. **Tarefa.** Toque na caixa de **Ligar para o fornecedor**: marca na hora. Volte para o editor ou olhe no PC: só aquele `[ ]` virou `[x]`, o resto do texto está igual. Toque de novo: desmarca.
6. **Mudou por fora.** Com a nota aberta em **Ler**, edite a mesma nota pelo PC ou outro app (acrescente uma linha) e depois toque numa caixa. O app não grava, mostra a versão nova e avisa; a sua linha nova continua no arquivo.
7. **Nota vazia.** Abra uma nota vazia e toque em **Ler**: diz que está vazia. Leia nos temas claro e escuro.
8. **Imagem.** Ponha um `foto.png` pequeno dentro do cofre e escreva `![minha foto](foto.png)` numa nota: aparece a imagem. Com `![x](https://exemplo.com/a.png)` ou um arquivo de fora do cofre aparece só "imagem: a.png".
9. **Sensação.** Uma nota grande (algumas telas) abre rápido? Rola bem?

Me conte o que não saiu como descrito (número do item, modelo e Android).
