//! Assinatura das atualizações (R16): `chave-nova` e `release`.
//!
//! O formato e a conferência ficam em `src/atualizacao.rs` (incluído por `#[path]`), o mesmo
//! código que o app usa para recusar o que não foi assinado. A chave privada mora num arquivo
//! FORA do repositório, escolhido pelo dono: nunca é impressa, nem entra em argumento de
//! linha de comando, nem em variável de ambiente (só o CAMINHO do arquivo pode ir no ambiente).

use crate::atualizacao::{
    self, CHAVE_PUBLICA_EMBUTIDA, ler_chave_privada, ler_chave_publica, texto_chave_privada,
    texto_chave_publica, verificar_lista,
};
use crate::ensure;
use crate::util::{Args, Result, fail, root};
use ed25519_dalek::SigningKey;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Variável com o CAMINHO do arquivo da chave privada (nunca a chave em si).
pub const VARIAVEL_CHAVE: &str = "RUSTIDIAN_CHAVE_PRIVADA";

const CABECALHO_PUBLICA: &str = "# Chave pública de assinatura das atualizações do Rustidian (R16), embutida no app.\n\
# Gerada por `cargo xtask chave-nova`. Trocar esta chave invalida as assinaturas antigas.\n";

fn arquivo_publico() -> PathBuf {
    root().join("src/atualizacao/chave-publica.txt")
}

/// A chave privada do arquivo dado por `--chave` ou, na falta, pelo caminho em
/// `RUSTIDIAN_CHAVE_PRIVADA`. `None` só se nenhum dos dois foi pedido; pedido e ilegível é erro
/// (um release que o dono quis assinar nunca sai sem assinatura por engano).
pub fn carregar_chave(opcao: Option<&str>) -> Result<Option<SigningKey>> {
    let caminho = match opcao {
        Some(caminho) => caminho.to_owned(),
        None => match std::env::var(VARIAVEL_CHAVE) {
            Ok(caminho) if !caminho.trim().is_empty() => caminho,
            _ => return Ok(None),
        },
    };
    let texto = fs::read_to_string(&caminho)
        .map_err(|e| format!("Não consegui ler o arquivo da chave privada ({caminho}): {e}"))?;
    Ok(Some(ler_chave_privada(&texto)?))
}

/// Escreve o arquivo da chave privada sem sobrescrever e, no Unix, só para o dono (0600).
fn gravar_privada(caminho: &Path, texto: &str) -> Result<()> {
    let mut opcoes = fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opcoes.mode(0o600);
    }
    let mut arquivo = opcoes
        .open(caminho)
        .map_err(|e| format!("{}: {e}", caminho.display()))?;
    arquivo.write_all(texto.as_bytes())?;
    arquivo.sync_all()?;
    Ok(())
}

/// A pasta onde o arquivo ficaria, resolvida (precisa existir): serve para saber se cai no repo.
fn pasta_resolvida(arquivo: &Path) -> Result<PathBuf> {
    let absoluto = std::path::absolute(arquivo)?;
    let pasta = absoluto
        .parent()
        .ok_or("Caminho da chave privada sem pasta")?;
    fs::canonicalize(pasta).map_err(|e| {
        format!(
            "A pasta da chave privada precisa existir ({}): {e}",
            pasta.display()
        )
        .into()
    })
}

/// Gera o par: a privada em `privada` (fora de `raiz`), a pública em `publica`. Devolve a
/// pública em hexadecimal. Nada da privada sai daqui além do arquivo.
pub fn criar_par(privada: &Path, publica: &Path, raiz: &Path, trocar: bool) -> Result<String> {
    let pasta = pasta_resolvida(privada)?;
    let raiz = fs::canonicalize(raiz)?;
    ensure!(
        !pasta.starts_with(&raiz),
        "A chave privada não pode ficar dentro do repositório ({}). Escolha uma pasta fora dele.",
        raiz.display()
    );
    ensure!(
        !privada.exists(),
        "Já existe um arquivo em {}: não sobrescrevo chave privada.",
        privada.display()
    );
    if let Ok(texto) = fs::read_to_string(publica)
        && matches!(ler_chave_publica(&texto), Ok(Some(_)))
    {
        ensure!(
            trocar,
            "O app já tem uma chave pública embutida. Trocar invalida todas as assinaturas já \
             publicadas; se é isso mesmo que você quer, repita com --trocar."
        );
    }
    let mut semente = [0u8; 32];
    getrandom::fill(&mut semente).map_err(|e| format!("Sem fonte de aleatoriedade: {e}"))?;
    let chave = SigningKey::from_bytes(&semente);
    semente.fill(0);
    gravar_privada(privada, &texto_chave_privada(&chave))?;
    let linha = texto_chave_publica(&chave.verifying_key());
    if let Err(erro) = fs::write(publica, format!("{CABECALHO_PUBLICA}{linha}")) {
        let _ = fs::remove_file(privada); // acabou de ser criada: não deixa meia-geração
        return fail(format!("{}: {erro}", publica.display()));
    }
    Ok(atualizacao::hex(chave.verifying_key().as_bytes()))
}

/// `cargo xtask chave-nova <arquivo-da-chave-privada> [--trocar]`.
pub fn chave_nova(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &["trocar"])?;
    args.only(&["trocar"])?;
    ensure!(
        args.positional.len() == 1,
        "Uso: cargo xtask chave-nova <arquivo da chave privada, FORA do repositório> [--trocar]"
    );
    let privada = PathBuf::from(&args.positional[0]);
    let publica = arquivo_publico();
    let hex = criar_par(&privada, &publica, &root(), args.flag("trocar"))?;
    println!("Chave privada gravada em: {}", privada.display());
    println!(
        "  Guarde uma cópia em lugar seguro (gerenciador de senhas ou pen drive). Sem ela não dá"
    );
    println!("  para assinar atualizações; com ela, qualquer um assina por você. Nunca commite.");
    println!("Chave pública ({hex}) gravada em: {}", publica.display());
    println!("  Faça commit desse arquivo e recompile o app: é ele que embute a chave.");
    println!(
        "Para assinar um release: cargo xtask release --dir <pasta> --chave {}",
        privada.display()
    );
    Ok(())
}

/// O que aconteceu num `release`.
#[derive(Debug, PartialEq, Eq)]
pub struct Resultado {
    pub arquivos: usize,
    pub assinado: bool,
}

/// Gera `SHA256SUMS` da pasta e, com chave, a assinatura. `embutida` é a chave pública do
/// app: se existir e não for a par da privada, recusa (o app recusaria o release inteiro).
pub fn assinar_pasta(
    pasta: &Path,
    chave: Option<&SigningKey>,
    embutida: Option<&ed25519_dalek::VerifyingKey>,
) -> Result<Resultado> {
    ensure!(pasta.is_dir(), "{} não é uma pasta", pasta.display());
    if let (Some(chave), Some(embutida)) = (chave, embutida) {
        ensure!(
            chave.verifying_key() == *embutida,
            "Esta chave privada não é a par da chave pública embutida no app \
             (src/atualizacao/chave-publica.txt): o instalador recusaria este release."
        );
    }
    let assinado = atualizacao::gravar_somas(pasta, chave)?;
    let arquivos =
        atualizacao::ler_somas(&fs::read_to_string(pasta.join(atualizacao::NOME_SOMAS))?)
            .map_err(|e| e.to_string())?
            .len();
    if let (true, Some(embutida)) = (assinado, embutida) {
        // Confere o que acabou de gravar do jeito que o app vai conferir.
        verificar_lista(pasta, embutida).map_err(|e| format!("A conferência final falhou: {e}"))?;
    }
    Ok(Resultado { arquivos, assinado })
}

/// `cargo xtask release --dir <pasta> [--chave <arquivo>]`.
pub fn release(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["dir", "chave"])?;
    let pasta = args.require_path("dir")?;
    let chave = carregar_chave(args.opt("chave"))?;
    let embutida = ler_chave_publica(CHAVE_PUBLICA_EMBUTIDA).map_err(|e| e.to_string())?;
    let resultado = assinar_pasta(&pasta, chave.as_ref(), embutida.as_ref())?;
    println!(
        "{} arquivos em {}/{}",
        resultado.arquivos,
        pasta.display(),
        atualizacao::NOME_SOMAS
    );
    if resultado.assinado {
        println!(
            "Assinado: {}/{}",
            pasta.display(),
            atualizacao::NOME_ASSINATURA
        );
        if embutida.is_none() {
            eprintln!(
                "AVISO: o app ainda não embute chave pública (rode `cargo xtask chave-nova`); o \
                 instalador compilado deste código vai recusar esta assinatura."
            );
        }
    } else {
        eprintln!(
            "AVISO: este release saiu SEM ASSINATURA (nenhuma chave privada: use --chave <arquivo> \
             ou a variável {VARIAVEL_CHAVE}). O instalador do app vai recusá-lo, a não ser com \
             --sem-assinatura (só desenvolvedor)."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn chave_de_teste(nome: &str) -> SigningKey {
        let semente: [u8; 32] = Sha256::digest(format!("xtask-r16/{nome}")).into();
        SigningKey::from_bytes(&semente)
    }

    #[test]
    fn chave_nova_grava_o_par_fora_do_repo_e_nao_sobrescreve() {
        let raiz = tempfile::tempdir().unwrap();
        let fora = tempfile::tempdir().unwrap();
        let publica = raiz.path().join("chave-publica.txt");
        fs::write(&publica, "rustidian-chave-publica-v1 NENHUMA\n").unwrap();
        let privada = fora.path().join("minha.chave");
        let hex = criar_par(&privada, &publica, raiz.path(), false).unwrap();
        let par = ler_chave_privada(&fs::read_to_string(&privada).unwrap()).unwrap();
        assert_eq!(atualizacao::hex(par.verifying_key().as_bytes()), hex);
        let gravada = fs::read_to_string(&publica).unwrap();
        assert!(gravada.contains(&hex) && !gravada.contains(&texto_chave_privada(&par)));
        assert!(ler_chave_publica(&gravada).unwrap().is_some());
        // Já existe: não sobrescreve a privada.
        let erro = criar_par(&privada, &publica, raiz.path(), true).unwrap_err();
        assert!(erro.to_string().contains("não sobrescrevo"));
        // Já há chave pública real: só com --trocar.
        let outra = fora.path().join("outra.chave");
        let erro = criar_par(&outra, &publica, raiz.path(), false).unwrap_err();
        assert!(erro.to_string().contains("--trocar"));
        assert!(!outra.exists());
        criar_par(&outra, &publica, raiz.path(), true).unwrap();
        assert!(outra.exists());
    }

    #[test]
    fn chave_nova_recusa_pasta_dentro_do_repo() {
        let raiz = tempfile::tempdir().unwrap();
        fs::create_dir(raiz.path().join("sub")).unwrap();
        let publica = raiz.path().join("chave-publica.txt");
        let privada = raiz.path().join("sub/minha.chave");
        let erro = criar_par(&privada, &publica, raiz.path(), false).unwrap_err();
        assert!(erro.to_string().contains("dentro do repositório"), "{erro}");
        assert!(!privada.exists() && !publica.exists());
    }

    #[cfg(unix)]
    #[test]
    fn chave_privada_so_para_o_dono() {
        use std::os::unix::fs::PermissionsExt;
        let raiz = tempfile::tempdir().unwrap();
        let fora = tempfile::tempdir().unwrap();
        let privada = fora.path().join("k");
        criar_par(&privada, &raiz.path().join("p.txt"), raiz.path(), false).unwrap();
        let modo = fs::metadata(&privada).unwrap().permissions().mode();
        assert_eq!(modo & 0o777, 0o600);
    }

    #[test]
    fn release_com_chave_assina_e_sem_chave_avisa() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.tar.gz"), b"um").unwrap();
        fs::write(dir.path().join("b.deb"), b"dois").unwrap();
        let chave = chave_de_teste("a");
        let publica = chave.verifying_key();
        let r = assinar_pasta(dir.path(), Some(&chave), Some(&publica)).unwrap();
        assert_eq!(
            r,
            Resultado {
                arquivos: 2,
                assinado: true
            }
        );
        assert_eq!(verificar_lista(dir.path(), &publica), Ok(2));
        // Sem chave: gera a lista, apaga a assinatura velha e diz que não assinou.
        let r = assinar_pasta(dir.path(), None, Some(&publica)).unwrap();
        assert!(!r.assinado);
        assert!(!dir.path().join(atualizacao::NOME_ASSINATURA).exists());
        // Chave de outra pessoa contra a chave embutida: recusa antes de gravar a assinatura.
        let intruso = chave_de_teste("intruso");
        let erro = assinar_pasta(dir.path(), Some(&intruso), Some(&publica)).unwrap_err();
        assert!(erro.to_string().contains("não é a par"));
        assert!(!dir.path().join(atualizacao::NOME_ASSINATURA).exists());
    }

    #[test]
    fn chave_pedida_e_ilegivel_e_erro() {
        let erro = carregar_chave(Some("/nao/existe/chave")).unwrap_err();
        assert!(erro.to_string().contains("chave privada"));
    }
}
