# Servidor e backup (re-servidor)

Estado em 15/11/2026. Nenhum segredo neste arquivo.

## nota-servidor
- **Binário:** `/usr/local/bin/nota-servidor`, compilado no próprio servidor a partir deste repositório (`~/rustidian`, `cargo build --release -p nota-servidor`).
- **Serviço:** `nota-servidor.service`, roda como o usuário `replay`. Tem `Restart=always`, `ProtectSystem=strict` e pode escrever só em `~/nota-servidor/dados`.
- **Rede:** escuta só em `100.122.119.49:8765`, o IP da Tailscale. Pela rede local (192.168.15.59) não responde. Sem token, responde 401.
- **Teste de queda:** com o processo morto à força, o systemd religou (NRestarts=1).
- **Atualizar:** `cd ~/rustidian && git pull && cargo build --release -p nota-servidor && sudo install -m 755 target/release/nota-servidor /usr/local/bin/ && sudo systemctl restart nota-servidor`.
- **Token de um aparelho:** `nota-servidor token novo <aparelho> --dados ~/nota-servidor/dados`. Ele é impresso uma única vez: vai direto para o keyring do aparelho e nunca para chat ou arquivo do repositório.

## Backup (rustic + rclone → Google Drive)
- **rclone** (apt, 1.60): o remoto `gdrive` usa o escopo `drive.file`, então só enxerga os arquivos que ele mesmo criou. A configuração fica em `~/.config/rclone/rclone.conf` (600).
- **rustic** 0.11.4, binário oficial com sha256 conferido, em `/usr/local/bin/rustic`. O perfil fica em `~/.config/rustic/rustic.toml`.
  - Repositório: `rclone:gdrive:backup-notas`.
  - Origem: `~/nota-servidor/dados`, que já está cifrado ponta a ponta; o rustic cifra de novo.
  - Senha: `~/.config/rustic/senha` (600). Foi gerada no servidor; o dono guarda uma cópia no gerenciador de senhas.
- **Timers:**
  - `backup-notas.timer`, todo dia às 03:30: `rustic backup` + `rustic forget --prune`. Retenção de 7 diários, 4 semanais e 12 mensais. Com `Persistent=true`, roda na volta se o servidor estava desligado.
  - `backup-notas-conferir.timer`, aos domingos às 04:30: `rustic check --read-data-subset=5%`.
- **Prova de 15/11:** o snapshot `teste`, de `~/instalar/teste-backup`, foi restaurado numa pasta nova e ficou idêntico (`diff -r`).
- **Restaurar tudo** (servidor perdido):
  1. instalar rclone e rustic;
  2. refazer o login do rclone, com escopo `drive.file`;
  3. pôr a senha em `~/.config/rustic/senha`;
  4. `rustic restore latest:/home/replay/nota-servidor/dados <pasta>`;
  5. subir o nota-servidor apontando para a pasta;
  6. nos aparelhos, a frase-senha do cofre abre tudo.

## Pendências
- Teste de reinício completo do servidor (reboot), quando o dono puder: o Rê roda no mesmo notebook.
- Teste mensal de restauração automática (hoje é manual).
- Reimplantar depois da R5 (rotas `/v1/chave`).
