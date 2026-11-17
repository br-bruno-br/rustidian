//! `package-appimage`: o executável verificado como AppImage x86_64
//! (antes `scripts/package-appimage.py`). Precisa de `appimagetool` no PATH ou em APPIMAGETOOL.

use crate::ensure;
use crate::util::{
    Args, Result, copy_tree, fail, root, run_inherit, set_mode, stdout_text, which,
    write_sha256_sidecar,
};
use std::{fs, path::PathBuf, process::Command};

/// O AppStream quer o nome do arquivo de metainfo igual ao id do componente e um `.desktop` com esse nome.
pub const APP_ID: &str = "io.github.masihmoafi.rustidian";

pub const APPRUN: &str = r#"#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
for folder in /usr/local/node/bin "$HOME/.local/bin"; do
    case ":$PATH:" in
        *":$folder:"*) ;;
        *) if [ -d "$folder" ]; then PATH="$folder${PATH:+:$PATH}"; fi ;;
    esac
done
export PATH
exec "$HERE/usr/bin/rustidian" "$@"
"#;

pub const DESKTOP: &str = "[Desktop Entry]\nType=Application\nName=Rustidian\nComment=Local Markdown notes and source code\n\
Exec=rustidian %f\nIcon=rustidian\nTerminal=false\nCategories=Office;TextEditor;\n\
Keywords=Markdown;Notes;Code;\nStartupWMClass=rustidian\n";

pub fn run(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["binary", "output", "notices"])?;
    let binary = fs::canonicalize(args.require_path("binary")?)?;
    let output = args.require_path("output")?;
    let notices = args.require_path("notices")?;
    let root = root();
    let identity = stdout_text(Command::new(&binary).arg("--version"))?;
    ensure!(
        identity.starts_with("Rustidian "),
        "Expected a Rustidian executable"
    );
    let tool: PathBuf = match std::env::var_os("APPIMAGETOOL").filter(|v| !v.is_empty()) {
        Some(tool) => PathBuf::from(tool),
        None => match which("appimagetool") {
            Some(tool) => tool,
            None => return fail("appimagetool not found"),
        },
    };
    if let Some(folder) = output.parent() {
        fs::create_dir_all(folder)?;
    }
    let scratch = tempfile::Builder::new()
        .prefix("rustidian-appdir-")
        .tempdir()?;
    let appdir = scratch.path().join("Rustidian.AppDir");
    for folder in [
        "usr/bin",
        "usr/share/rustidian",
        "usr/share/doc/rustidian",
        "usr/share/metainfo",
        "usr/share/applications",
    ] {
        fs::create_dir_all(appdir.join(folder))?;
    }
    fs::copy(&binary, appdir.join("usr/bin/rustidian"))?;
    set_mode(&appdir.join("usr/bin/rustidian"), 0o755)?;
    copy_tree(
        &root.join("demo-vault"),
        &appdir.join("usr/share/rustidian/demo-vault"),
    )?;
    let documents = appdir.join("usr/share/doc/rustidian");
    fs::copy(root.join("LICENSE"), documents.join("copyright"))?;
    fs::copy(&notices, documents.join("THIRD_PARTY_NOTICES.txt"))?;
    crate::package_deb::copy_font_licenses(&root.join("assets/fonts"), &documents)?;
    let metainfo = fs::read_to_string(root.join("packaging/rustidian.metainfo.xml"))?
        .replace(">rustidian.desktop<", &format!(">{APP_ID}.desktop<"));
    fs::write(
        appdir.join(format!("usr/share/metainfo/{APP_ID}.appdata.xml")),
        metainfo,
    )?;
    fs::copy(
        root.join("docs/brand/store-icon.png"),
        appdir.join("rustidian.png"),
    )?;
    let desktop = appdir.join(format!("{APP_ID}.desktop"));
    fs::write(&desktop, DESKTOP)?;
    fs::copy(
        &desktop,
        appdir.join(format!("usr/share/applications/{APP_ID}.desktop")),
    )?;
    fs::write(appdir.join("AppRun"), APPRUN)?;
    set_mode(&appdir.join("AppRun"), 0o755)?;
    run_inherit(Command::new("desktop-file-validate").arg(&desktop))?;
    run_inherit(
        Command::new(&tool)
            .arg(&appdir)
            .arg(std::path::absolute(&output)?)
            .env("ARCH", "x86_64")
            .env("APPIMAGE_EXTRACT_AND_RUN", "1"),
    )?;
    write_sha256_sidecar(&output)?;
    println!("{}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apprun_poe_as_pastas_do_agente_no_path() {
        assert!(APPRUN.contains("/usr/local/node/bin"));
        assert!(APPRUN.ends_with("exec \"$HERE/usr/bin/rustidian\" \"$@\"\n"));
        assert!(DESKTOP.contains("StartupWMClass=rustidian"));
    }
}
