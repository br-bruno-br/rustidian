//! `package-deb`: pacote Debian amd64 sem mexer no executável verificado
//! (antes `scripts/package-deb.py`). Precisa de `readelf`, `desktop-file-validate` e `dpkg-deb`.

use crate::ensure;
use crate::util::{
    Args, Result, copy_tree, fail, root, run_checked, run_inherit, set_mode, stdout_text,
    write_sha256_sidecar,
};
use regex::Regex;
use std::{fs, io::Read, path::Path, process::Command};

/// O lançador de `/usr/bin/rustidian`: prepara um demo gravável no primeiro uso e executa o app.
/// Em shell (não depende mais do Python do sistema).
const LAUNCHER: &str = r#"#!/bin/sh
prefix="$(dirname "$(dirname "$(readlink -f "$0")")")"
if [ "$#" -eq 0 ] && [ -z "${RUSTIDIAN_DEFAULT_VAULT+x}" ] && [ -z "${RUST_NOTES_DEFAULT_VAULT+x}" ]; then
    data="${XDG_DATA_HOME:-$HOME/.local/share}/rustidian"
    demo="$data/demo-vault"
    if [ ! -e "$demo" ]; then
        if ! { mkdir -p "$data" && cp -R "$prefix/share/rustidian/demo-vault" "$demo"; }; then
            echo "Cannot prepare the writable demo vault" >&2
            exit 1
        fi
    fi
    RUSTIDIAN_DEFAULT_VAULT="$demo"
    export RUSTIDIAN_DEFAULT_VAULT
fi
exec "$prefix/lib/rustidian/rustidian" "$@"
"#;

const DEPENDS_TAIL: &str = "libstdc++6, libgcc-s1, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrandr2, libxinerama1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-egl1, libegl1, libgl1, libglib2.0-bin, dbus-bin, gsettings-desktop-schemas, zenity";

pub fn run(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["binary", "output", "notices", "revision"])?;
    let binary = fs::canonicalize(args.require_path("binary")?)?;
    let output = args.require_path("output")?;
    let notices = args.require_path("notices")?;
    let revision: u32 = args.opt("revision").unwrap_or("1").parse()?;
    let root = root();
    let mut header = [0u8; 20];
    fs::File::open(&binary)?.read_exact(&mut header)?;
    ensure!(
        header[..6] == *b"\x7fELF\x02\x01" && header[18..20] == *b"\x3e\x00",
        "Expected the verified Linux amd64 ELF executable"
    );
    let identity = stdout_text(Command::new(&binary).arg("--version"))?;
    ensure!(
        identity.starts_with("Rustidian ") && revision >= 1,
        "Expected a Rustidian executable and positive package revision"
    );
    let info = stdout_text(
        Command::new("readelf")
            .args(["--version-info", "--wide"])
            .arg(&binary),
    )?;
    let glibc = glibc_minimum(&info)?;
    let upstream = identity
        .split_once(' ')
        .map(|(_, version)| version)
        .unwrap_or_default()
        .replace("-alpha.", "~alpha.");
    ensure!(
        Regex::new(r"^[0-9][A-Za-z0-9.+~]*$")?.is_match(&upstream),
        "Unsupported version string: {upstream}"
    );
    ensure!(notices.is_file(), "Third-party notices are required");
    if let Some(folder) = output.parent() {
        fs::create_dir_all(folder)?;
    }
    let scratch = tempfile::Builder::new()
        .prefix("rustidian-deb-")
        .tempdir()?;
    let package = scratch.path();
    for folder in [
        "DEBIAN",
        "usr/bin",
        "usr/lib/rustidian",
        "usr/share/rustidian",
        "usr/share/applications",
        "usr/share/icons/hicolor/256x256/apps",
        "usr/share/doc/rustidian",
    ] {
        fs::create_dir_all(package.join(folder))?;
    }
    let installed = package.join("usr/lib/rustidian/rustidian");
    fs::copy(&binary, &installed)?;
    set_mode(&installed, 0o755)?;
    let launcher = package.join("usr/bin/rustidian");
    fs::write(&launcher, LAUNCHER)?;
    set_mode(&launcher, 0o755)?;
    copy_tree(
        &root.join("demo-vault"),
        &package.join("usr/share/rustidian/demo-vault"),
    )?;
    fs::copy(
        root.join("docs/brand/store-icon.png"),
        package.join("usr/share/icons/hicolor/256x256/apps/rustidian.png"),
    )?;
    let documents = package.join("usr/share/doc/rustidian");
    fs::copy(root.join("LICENSE"), documents.join("copyright"))?;
    fs::copy(&notices, documents.join("THIRD_PARTY_NOTICES.txt"))?;
    copy_font_licenses(&root.join("assets/fonts"), &documents)?;
    let desktop = package.join("usr/share/applications/rustidian.desktop");
    fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName=Rustidian\nComment=Local Markdown notes and source code\nExec=rustidian %f\nIcon=rustidian\nTerminal=false\nCategories=Office;TextEditor;\nKeywords=Markdown;Notes;Code;\nStartupWMClass=rustidian\nStartupNotify=true\nMimeType={}\n",
            crate::install_linux::MIME_TYPES
        ),
    )?;
    run_inherit(Command::new("desktop-file-validate").arg(&desktop))?;
    let size: u64 = walkdir::WalkDir::new(package.join("usr"))
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    let dependencies = format!("libc6 (>= {glibc}), {DEPENDS_TAIL}");
    fs::write(
        package.join("DEBIAN/control"),
        format!(
            "Package: rustidian\nVersion: {upstream}-{revision}\nArchitecture: amd64\nSection: editors\nPriority: optional\nMaintainer: Rustidian contributors\nInstalled-Size: {}\nDepends: {dependencies}\nHomepage: https://github.com/MasihMoafi/rustidian\nDescription: Native Markdown notebook and source text editor\n Local vaults, linked notes and native editing.\n Obsidian community plugins are not supported.\n",
            size.div_ceil(1024)
        ),
    )?;
    let epoch = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| "0".into());
    let absolute_output = std::path::absolute(&output)?;
    run_checked(
        Command::new("dpkg-deb")
            .args(["--root-owner-group", "-Zgzip", "-z3", "--build"])
            .arg(package)
            .arg(&absolute_output)
            .env("SOURCE_DATE_EPOCH", epoch),
    )?;
    write_sha256_sidecar(&output)?;
    println!("{}", output.display());
    Ok(())
}

/// A versão mínima do glibc: o maior `GLIBC_x.y` do executável, nunca abaixo de 2.39.
pub fn glibc_minimum(version_info: &str) -> Result<String> {
    let found: Vec<Vec<u32>> = Regex::new(r"\bGLIBC_(\d+(?:\.\d+)+)\b")?
        .captures_iter(version_info)
        .map(|capture| {
            capture[1]
                .split('.')
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .collect();
    if found.is_empty() {
        return fail("Cannot determine the executable's glibc requirement");
    }
    let mut best: Vec<u32> = vec![2, 39];
    for parts in found {
        if parts > best {
            best = parts;
        }
    }
    Ok(best
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("."))
}

pub fn copy_font_licenses(fonts: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(fonts)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name.starts_with("LICENSE-") && name.ends_with(".txt") {
            fs::copy(&path, destination.join(name))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glibc_minimo_nunca_fica_abaixo_de_2_39() {
        assert_eq!(glibc_minimum("GLIBC_2.17 GLIBC_2.2.5").unwrap(), "2.39");
        assert_eq!(glibc_minimum("x GLIBC_2.40 y GLIBC_2.39").unwrap(), "2.40");
        assert_eq!(glibc_minimum("GLIBC_2.100").unwrap(), "2.100");
        assert!(glibc_minimum("nada").is_err());
    }

    #[test]
    fn lancador_e_shell_sem_python() {
        assert!(LAUNCHER.starts_with("#!/bin/sh\n"));
        assert!(!LAUNCHER.contains("python"));
        assert!(!DEPENDS_TAIL.contains("python3"));
    }
}
