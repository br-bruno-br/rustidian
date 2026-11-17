//! `test-bundle` e `test-deb`: exercitam o pacote portátil e o `.deb` reais, sem tocar no perfil
//! do usuário (antes `tests/linux_bundle.py`, `tests/debian_package.py` e `tests/launcher_path.py`;
//! este último virou teste unitário em `src/install_linux.rs`). Só no Linux.

use crate::ensure;
use crate::util::{
    Args, Result, fail, json_line, root, run_checked, sha256_bytes, sha256_file, stdout_text,
};
use regex::Regex;
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn xtask(root: &Path, args: &[&str]) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(args).current_dir(root);
    Ok(command)
}

pub fn bundle(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["binary", "archive"])?;
    let binary = args.require_path("binary")?;
    let root = root();
    let scratch = tempfile::Builder::new()
        .prefix("rustidian-bundle-")
        .tempdir()?;
    let base = scratch.path();
    let archive = match args.path("archive") {
        Some(archive) => fs::canonicalize(archive)?,
        None => {
            let archive = base.join("rustidian-linux-x86_64.tar.gz");
            run_checked(&mut xtask(
                &root,
                &[
                    "package-linux",
                    "--binary",
                    &binary.to_string_lossy(),
                    "--output",
                    &archive.to_string_lossy(),
                ],
            )?)?;
            archive
        }
    };
    let mut sidecar = archive.clone().into_os_string();
    sidecar.push(".sha256");
    let checksum = fs::read_to_string(sidecar)?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    ensure!(
        sha256_file(&archive)? == checksum,
        "O checksum do arquivo não confere"
    );
    let bundle = base.join("extracted bundle");
    {
        let mut package =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&archive)?));
        package.set_preserve_permissions(true);
        package.unpack(&bundle)?;
    }
    ensure!(
        sha256_file(&bundle.join("rustidian"))? == sha256_file(&binary)?,
        "O executável do pacote difere do verificado"
    );
    ensure!(
        fs::read(bundle.join("demo-vault/Home.md"))? == fs::read(root.join("demo-vault/Home.md"))?,
        "O demo do pacote difere do repositório"
    );
    let prefix = base.join("Local \"notes\" $safe %app `quoted`");
    let vault = base.join("Vault سلام");
    fs::create_dir_all(&vault)?;
    let note = vault.join("Keep.md");
    fs::write(&note, "# Keep\n\nسلام [[Other]]\n")?;
    let original = fs::read(&note)?;
    let unrelated = base.join("unrelated XDG data");
    let install = |bundle: &Path| -> Result<std::process::Output> {
        Ok(Command::new(bundle.join("rustidian"))
            .args(["--install", "--prefix"])
            .arg(&prefix)
            .arg("--default-vault")
            .arg(&vault)
            .env("XDG_DATA_HOME", &unrelated)
            .output()?)
    };
    let first = install(&bundle)?;
    ensure!(
        first.status.success(),
        "A instalação falhou: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let launcher = prefix.join("bin/rustidian");
    ensure!(
        stdout_text(Command::new(&launcher).arg("--version"))?.starts_with("Rustidian "),
        "O lançador não responde --version"
    );
    ensure!(
        stdout_text(Command::new(&launcher).arg("--check-vault").arg(&vault))?.contains("notes=1"),
        "O lançador não abre o cofre"
    );
    ensure!(fs::read(&note)? == original, "A instalação mexeu na nota");
    let demo = prefix.join("lib/rustidian/demo-vault/Home.md");
    fs::write(&demo, "User demo edits stay here\n")?;
    ensure!(install(&bundle)?.status.success(), "A reinstalação falhou");
    ensure!(
        fs::read_to_string(&demo)? == "User demo edits stay here\n",
        "A reinstalação apagou as edições do demo"
    );
    let entry = prefix.join("share/applications/rustidian.desktop");
    run_checked(Command::new("desktop-file-validate").arg(&entry))?;
    let registered: BTreeSet<String> = fs::read_to_string(&entry)?
        .lines()
        .find_map(|line| line.strip_prefix("MimeType="))
        .unwrap_or_default()
        .split(';')
        .map(str::to_owned)
        .collect();
    for (extension, contents) in [
        ("rs", "fn main() {}\n"),
        ("c", "int main(void) { return 0; }\n"),
        (
            "cpp",
            "#include <iostream>\nint main() { std::cout << \"hello\"; }\n",
        ),
        ("py", "print(\"hello\")\n"),
    ] {
        let source = base.join(format!("Open with.{extension}"));
        fs::write(&source, contents)?;
        let detected = stdout_text(
            Command::new("xdg-mime")
                .args(["query", "filetype"])
                .arg(&source),
        )?;
        ensure!(
            registered.contains(&detected),
            "Source is missing from Open With: {extension} {detected}"
        );
    }
    // Uma substituição inválida tem de falhar antes de mexer no app instalado.
    let protected = fs::read(prefix.join("lib/rustidian/rustidian"))?;
    fs::rename(
        bundle.join("rustidian.png"),
        bundle.join("missing-icon.png"),
    )?;
    let failure = install(&bundle)?;
    ensure!(!failure.status.success(), "Um pacote incompleto foi aceito");
    ensure!(
        fs::read(prefix.join("lib/rustidian/rustidian"))? == protected,
        "A instalação inválida mudou o app instalado"
    );
    ensure!(
        fs::read(&note)? == original,
        "A instalação inválida mexeu na nota"
    );
    ensure!(
        !unrelated.exists(),
        "--prefix must isolate the whole installation"
    );
    println!(
        "Package checks passed: checksum, exact demo, quoted paths, CLI, reinstall, desktop entry and failed replacement."
    );
    Ok(())
}

fn field(package: &Path, name: &str) -> Result<String> {
    stdout_text(
        Command::new("dpkg-deb")
            .arg("--field")
            .arg(package)
            .arg(name),
    )
}

pub fn deb(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &["system"])?;
    args.only(&["binary", "notices", "package", "system"])?;
    let binary_path = args.require_path("binary")?;
    let notices = args.require_path("notices")?;
    let system = args.flag("system");
    let root = root();
    let original = sha256_file(&binary_path)?;
    let scratch = tempfile::Builder::new()
        .prefix("rustidian-debian-check-")
        .tempdir()?;
    let base = scratch.path();
    let build = |output: &Path, revision: Option<&str>| -> Result<()> {
        let mut command = xtask(
            &root,
            &[
                "package-deb",
                "--binary",
                &binary_path.to_string_lossy(),
                "--notices",
                &notices.to_string_lossy(),
                "--output",
                &output.to_string_lossy(),
            ],
        )?;
        if let Some(revision) = revision {
            command.args(["--revision", revision]);
        }
        run_checked(&mut command)?;
        Ok(())
    };
    let package: PathBuf = match args.path("package") {
        Some(package) => fs::canonicalize(package)?,
        None => {
            let package = base.join("rustidian.deb");
            build(&package, None)?;
            package
        }
    };
    let package_digest = sha256_file(&package)?;
    ensure!(
        field(&package, "Package")? == "rustidian",
        "Pacote com nome errado"
    );
    ensure!(
        field(&package, "Architecture")? == "amd64",
        "Pacote com arquitetura errada"
    );
    let extracted = base.join("extracted package");
    run_checked(
        Command::new("dpkg-deb")
            .arg("--extract")
            .arg(&package)
            .arg(&extracted),
    )?;
    let binary = extracted.join("usr/lib/rustidian/rustidian");
    ensure!(
        sha256_file(&binary)? == original,
        "O executável do pacote difere do verificado"
    );
    let depends = field(&package, "Depends")?;
    let Some(declared) = Regex::new(r"libc6 \(>= ([\d.]+)\)")?
        .captures(&depends)
        .map(|capture| capture[1].to_owned())
    else {
        return fail("The package must declare its glibc minimum");
    };
    let symbols = stdout_text(Command::new("objdump").arg("--dynamic-syms").arg(&binary))?;
    let required: BTreeSet<String> = Regex::new(r"GLIBC_([\d.]+)")?
        .captures_iter(&symbols)
        .map(|capture| capture[1].to_owned())
        .collect();
    for version in required {
        run_checked(Command::new("dpkg").args(["--compare-versions", &declared, "ge", &version]))?;
    }
    let launcher = extracted.join("usr/bin/rustidian");
    ensure!(
        stdout_text(Command::new(&launcher).arg("--version"))?.starts_with("Rustidian "),
        "O lançador do pacote não responde --version"
    );
    let home = base.join("isolated home");
    fs::create_dir(&home)?;
    let runtime = base.join("runtime");
    fs::create_dir(&runtime)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
    }
    let isolated = |command: &mut Command| {
        command
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", base.join("no-bus").display()),
            )
            .env_remove("RUSTIDIAN_DEFAULT_VAULT")
            .env_remove("RUST_NOTES_DEFAULT_VAULT");
    };
    // O primeiro lançamento real prepara um demo do usuário antes de a GUI falhar, certo, sem display.
    let mut first = Command::new(&launcher);
    isolated(&mut first);
    let first = first.output()?;
    ensure!(
        !first.status.success(),
        "A display is deliberately unavailable in this packaging check"
    );
    let demo = home.join("data/rustidian/demo-vault/Home.md");
    ensure!(
        fs::read(&demo)? == fs::read(root.join("demo-vault/Home.md"))?,
        "O demo preparado difere do repositório: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    fs::write(&demo, "User demo edits survive upgrades\n")?;
    let mut second = Command::new(&launcher);
    isolated(&mut second);
    let _ = second.output()?;
    ensure!(
        fs::read_to_string(&demo)? == "User demo edits survive upgrades\n",
        "O segundo lançamento apagou as edições do demo"
    );
    let vault = home.join("My vault");
    fs::create_dir(&vault)?;
    let note = vault.join("Keep.md");
    fs::write(&note, "# Keep\n\nسلام\n")?;
    let recovery = home.join("state/rustidian/recovery.json");
    fs::create_dir_all(recovery.parent().expect("pasta do recovery"))?;
    fs::write(&recovery, "{\"saved\":true}\n")?;
    let protected: Vec<(PathBuf, Vec<u8>)> = [&note, &demo, &recovery]
        .iter()
        .map(|path| Ok(((*path).clone(), fs::read(path)?)))
        .collect::<Result<_>>()?;
    let mut check = Command::new(&launcher);
    check.arg("--check-vault").arg(&vault);
    isolated(&mut check);
    ensure!(
        stdout_text(&mut check)?.contains("notes=1"),
        "--check-vault não achou a nota"
    );
    if system {
        let lifecycle = || -> Result<()> {
            run_checked(
                Command::new("sudo")
                    .args([
                        "apt-get",
                        "-o",
                        "Acquire::http::Proxy=DIRECT",
                        "-o",
                        "Acquire::https::Proxy=DIRECT",
                        "install",
                        "-y",
                        "--no-install-recommends",
                    ])
                    .arg(&package),
            )?;
            ensure!(
                stdout_text(Command::new("/usr/bin/rustidian").arg("--version"))?
                    .starts_with("Rustidian "),
                "O app instalado não responde --version"
            );
            let upgraded = base.join("rustidian-upgrade.deb");
            build(&upgraded, Some("2"))?;
            run_checked(
                Command::new("sudo")
                    .args(["dpkg", "--install"])
                    .arg(&upgraded),
            )?;
            ensure!(
                stdout_text(Command::new("dpkg-query").args([
                    "--show",
                    "--showformat=${Version}",
                    "rustidian"
                ]))?
                .ends_with("-2"),
                "A versão instalada não é a revisão 2"
            );
            run_checked(Command::new("sudo").args(["dpkg", "--remove", "rustidian"]))?;
            ensure!(
                !Path::new("/usr/bin/rustidian").exists(),
                "O app continuou instalado"
            );
            run_checked(Command::new("sudo").args(["dpkg", "--purge", "rustidian"]))?;
            Ok(())
        };
        let result = lifecycle();
        let _ = Command::new("sudo")
            .args(["dpkg", "--purge", "rustidian"])
            .output();
        result?;
    }
    ensure!(
        sha256_bytes(&fs::read(&package)?) == package_digest,
        "Lifecycle checks modified the delivery package"
    );
    for (path, expected) in &protected {
        ensure!(
            fs::read(path)? == *expected,
            "Os dados do usuário mudaram: {}",
            path.display()
        );
    }
    println!(
        "{}",
        json_line(&json!({
            "status": "passed",
            "system_install_upgrade_remove_purge": system,
            "exact_binary_sha256": original,
            "package_sha256": package_digest,
            "user_notes_recovery_and_demo_retained": true,
        }))
    );
    Ok(())
}
