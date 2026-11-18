//! `package-linux`: o executável verificado, o demo e as licenças num `.tar.gz` portátil
//! (antes `scripts/package-linux.py`). O instalador agora é o próprio `rustidian --install`.

use crate::ensure;
use crate::util::{Args, Result, root, stdout_text, write_sha256_sidecar};
use flate2::{Compression, GzBuilder};
use std::{fs, path::Path, process::Command};

pub fn run(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["binary", "output", "notices", "chave"])?;
    let binary = args.require_path("binary")?;
    let output = args.require_path("output")?;
    let root = root();
    let binary = fs::canonicalize(&binary).map_err(|e| format!("{}: {e}", binary.display()))?;
    let version = stdout_text(Command::new(&binary).arg("--version"))?;
    ensure!(
        version.starts_with("Rustidian "),
        "Expected a verified Rustidian executable"
    );
    if let Some(folder) = output.parent() {
        fs::create_dir_all(folder)?;
    }
    let scratch = tempfile::Builder::new()
        .prefix("rustidian-package-")
        .tempdir()?;
    let bundle = scratch.path();
    fs::copy(&binary, bundle.join("rustidian"))?;
    fs::copy(
        root.join("docs/brand/store-icon.png"),
        bundle.join("rustidian.png"),
    )?;
    fs::copy(root.join("LICENSE"), bundle.join("LICENSE"))?;
    if let Some(notices) = args.path("notices") {
        fs::copy(notices, bundle.join("THIRD_PARTY_NOTICES.txt"))?;
    }
    for entry in fs::read_dir(root.join("assets/fonts"))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name.starts_with("LICENSE-") && name.ends_with(".txt") {
            fs::copy(&path, bundle.join(name))?;
        }
    }
    crate::util::copy_tree(&root.join("demo-vault"), &bundle.join("demo-vault"))?;
    fs::write(
        bundle.join("START_HERE.txt"),
        format!(
            "{version}\n\nRun ./rustidian to try the included demo.\n\
Install to your applications menu: ./rustidian --install\n\
(it checks the signed SHA256SUMS in this folder; an unsigned developer build needs --sem-assinatura)\n\
Open a copied vault: ./rustidian \"/path/to/vault copy\"\n\n\
Linux desktop notebook. Existing Obsidian community plugins are unsupported.\n\
Report a problem: https://github.com/MasihMoafi/rustidian/issues/new/choose\n"
        ),
    )?;
    // R16: a lista de hashes do pacote vai DENTRO dele, assinada se houver chave (`--chave` ou
    // RUSTIDIAN_CHAVE_PRIVADA); `rustidian --install` confere isso antes de copiar qualquer coisa.
    sign_bundle(bundle, args.opt("chave"))?;
    let mtime = source_date_epoch();
    write_archive(bundle, &output, mtime)?;
    write_sha256_sidecar(&output)?;
    println!("{}", output.display());
    Ok(())
}

/// Grava `SHA256SUMS` (e a assinatura, com chave) na pasta do pacote.
fn sign_bundle(bundle: &Path, key: Option<&str>) -> Result<()> {
    let key = crate::assinatura::carregar_chave(key)?;
    let embedded =
        crate::atualizacao::ler_chave_publica(crate::atualizacao::CHAVE_PUBLICA_EMBUTIDA)
            .map_err(|e| e.to_string())?;
    let done = crate::assinatura::assinar_pasta(bundle, key.as_ref(), embedded.as_ref())?;
    if !done.assinado {
        eprintln!(
            "AVISO: pacote SEM ASSINATURA (sem chave privada); `rustidian --install` vai recusá-lo \
             sem --sem-assinatura."
        );
    }
    Ok(())
}

pub fn source_date_epoch() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Grava `bundle/*` num `.tar.gz` com dono `root`, data fixa e modos explícitos (reprodutível).
pub fn write_archive(bundle: &Path, output: &Path, mtime: u64) -> Result<()> {
    let file = fs::File::create(output)?;
    let encoder = GzBuilder::new()
        .mtime(mtime as u32)
        .write(file, Compression::best());
    let mut archive = tar::Builder::new(encoder);
    let walker = walkdir::WalkDir::new(bundle)
        .min_depth(1)
        .sort_by_file_name();
    for entry in walker {
        let entry = entry?;
        let relative = entry.path().strip_prefix(bundle)?;
        let name = relative.to_string_lossy().replace('\\', "/");
        let mut header = tar::Header::new_gnu();
        header.set_uid(0);
        header.set_gid(0);
        header.set_username("root")?;
        header.set_groupname("root")?;
        header.set_mtime(mtime);
        if entry.file_type().is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            archive.append_data(&mut header, format!("{name}/"), std::io::empty())?;
        } else {
            let data = fs::read(entry.path())?;
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if name == "rustidian" { 0o755 } else { 0o644 });
            header.set_size(data.len() as u64);
            archive.append_data(&mut header, &name, data.as_slice())?;
        }
    }
    archive.into_inner()?.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn arquivo_tem_dono_root_data_fixa_e_ordem_estavel() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("b");
        fs::create_dir_all(bundle.join("demo-vault")).unwrap();
        fs::write(bundle.join("rustidian"), b"bin").unwrap();
        fs::write(bundle.join("LICENSE"), b"mit").unwrap();
        fs::write(bundle.join("demo-vault/Home.md"), b"# Home").unwrap();
        let out = dir.path().join("o.tar.gz");
        write_archive(&bundle, &out, 0).unwrap();
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&out).unwrap()));
        let mut seen = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let header = entry.header();
            assert_eq!(header.uid().unwrap(), 0);
            assert_eq!(header.username().unwrap(), Some("root"));
            assert_eq!(header.mtime().unwrap(), 0);
            let mode = header.mode().unwrap();
            let mut body = String::new();
            let _ = entry.read_to_string(&mut body);
            seen.push((name, mode));
        }
        assert_eq!(
            seen,
            [
                ("LICENSE".to_owned(), 0o644),
                ("demo-vault/".to_owned(), 0o755),
                ("demo-vault/Home.md".to_owned(), 0o644),
                ("rustidian".to_owned(), 0o755),
            ]
        );
    }
}
