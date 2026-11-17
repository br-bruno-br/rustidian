//! `lint-packaging`: checagens rápidas dos arquivos de empacotamento, para o CI não achar
//! o que um minuto aqui acharia (antes `scripts/lint-packaging.py`). Cada regra nomeia a falha
//! de CI que a ensinou. Sai com 1 se houver FAIL; avisos não reprovam.

use crate::util::{Args, Result, bash, fail, relative_slash, root, which};
use regex::Regex;
use serde_yaml_ng::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[derive(Default)]
struct Report {
    failures: Vec<String>,
    warnings: Vec<String>,
}

impl Report {
    fn fail(&mut self, rule: &str, detail: impl AsRef<str>) {
        self.failures
            .push(format!("FAIL {rule}: {}", detail.as_ref()));
    }

    fn warn(&mut self, rule: &str, detail: impl AsRef<str>) {
        self.warnings
            .push(format!("warn {rule}: {}", detail.as_ref()));
    }
}

fn run(root: &Path, program: &Path, args: &[&str]) -> Result<Output> {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("Não foi possível iniciar {}: {e}", program.display()).into())
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn key<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(name)
}

fn string_of<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    key(value, name)?.as_str()
}

fn bash_check(script: &str) -> Result<Output> {
    let mut child = Command::new(bash())
        .arg("-n")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Não foi possível iniciar bash: {e}"))?;
    child
        .stdin
        .take()
        .expect("stdin em pipe")
        .write_all(script.as_bytes())?;
    Ok(child.wait_with_output()?)
}

fn check_snap(root: &Path, report: &mut Report) -> Result<()> {
    let data: Value = serde_yaml_ng::from_str(&read(&root.join("snap/snapcraft.yaml"))?)?;
    for name in [
        "name",
        "base",
        "confinement",
        "grade",
        "summary",
        "description",
        "parts",
        "apps",
    ] {
        if key(&data, name).is_none() {
            report.fail("snap-key", format!("snapcraft.yaml lacks '{name}'"));
        }
    }
    if let Some(icon) = string_of(&data, "icon")
        && !root.join(icon).is_file()
    {
        report.fail("snap-icon", format!("icon {icon} does not exist"));
    }
    let empty = serde_yaml_ng::Mapping::new();
    let apps = key(&data, "apps")
        .and_then(Value::as_mapping)
        .unwrap_or(&empty);
    for (app, spec) in apps {
        let app = app.as_str().unwrap_or_default();
        if !root.join(format!("snap/gui/{app}.desktop")).is_file() {
            report.fail("snap-desktop", format!("snap/gui/{app}.desktop is missing"));
        }
        if key(spec, "desktop").is_some_and(|value| !value.is_null()) {
            report.warn(
                "snap-desktop",
                format!("app {app} sets 'desktop:' beside snap/gui/{app}.desktop; keep one"),
            );
        }
    }
    let parts = key(&data, "parts")
        .and_then(Value::as_mapping)
        .unwrap_or(&empty);
    let cargo = read(&root.join("Cargo.toml"))?;
    let wanted_rust = Regex::new(r#"rust-version\s*=\s*"([\d.]+)""#)?;
    // 3 Nov CI: "'rustup' not found and part does not depend on a part named 'rust-deps'".
    for (name, part) in parts {
        let name = name.as_str().unwrap_or_default();
        if string_of(part, "plugin") == Some("rust") && !parts.contains_key("rust-deps") {
            report.fail(
                "snap-rust-plugin",
                format!(
                    "part {name} uses plugin: rust without a rust-deps part; the snapcraft rust plugin cannot find rustup. Install the toolchain in override-build instead."
                ),
            );
        }
        if cargo.contains("rust-version") && name == "rustidian" {
            let wanted = wanted_rust
                .captures(&cargo)
                .map(|capture| capture[1].to_owned())
                .unwrap_or_default();
            if !string_of(part, "override-build")
                .unwrap_or_default()
                .contains(&wanted)
            {
                report.fail(
                    "snap-rust-version",
                    format!("Cargo.toml needs Rust {wanted}; override-build does not install it"),
                );
            }
        }
    }
    for field in ["override-build", "override-pull"] {
        for (name, part) in parts {
            if let Some(script) = string_of(part, field) {
                let result = bash_check(script)?;
                if !result.status.success() {
                    report.fail(
                        "snap-shell",
                        format!(
                            "part {} {field}: {}",
                            name.as_str().unwrap_or_default(),
                            text(&result).1.trim()
                        ),
                    );
                }
            }
        }
    }
    let copy = Regex::new(r"(?:install -Dm\d+|cp -r)\s+(\S+)\s")?;
    for part in parts.values() {
        for line in string_of(part, "override-build")
            .unwrap_or_default()
            .lines()
        {
            if let Some(capture) = copy.captures(line) {
                let source = &capture[1];
                if !source.contains('$')
                    && !source.starts_with("target/")
                    && !source.starts_with('-')
                    && !root.join(source).exists()
                    && !source.contains('*')
                {
                    report.fail(
                        "snap-source-file",
                        format!("override-build copies {source}, which is not in the repo"),
                    );
                }
            }
        }
    }
    if let Some(snapcraft) = which("snapcraft") {
        let result = Command::new(snapcraft)
            .arg("expand-extensions")
            .current_dir(root)
            .output()?;
        if !result.status.success() {
            let (out, err) = text(&result);
            let message = if err.is_empty() { out } else { err };
            report.fail(
                "snap-schema",
                message.trim().lines().last().unwrap_or_default(),
            );
        }
    } else {
        report.warn(
            "snap-schema",
            "snapcraft not installed; schema check skipped",
        );
    }
    Ok(())
}

fn check_workflows(root: &Path, report: &mut Report) -> Result<()> {
    let pinned = Regex::new(r"uses:\s*([\w./-]+)@(\S+)")?;
    let sha = Regex::new(r"^[0-9a-f]{40}$")?;
    let mut files: Vec<PathBuf> = fs::read_dir(root.join(".github/workflows"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .collect();
    files.sort();
    for path in files {
        let content = read(&path)?;
        serde_yaml_ng::from_str::<Value>(&content)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        for (index, line) in content.lines().enumerate() {
            if let Some(capture) = pinned.captures(line)
                && !sha.is_match(&capture[2])
            {
                report.fail(
                    "workflow-pin",
                    format!(
                        "{name}:{} {}@{} is not pinned to a commit SHA",
                        index + 1,
                        &capture[1],
                        &capture[2]
                    ),
                );
            }
        }
    }
    Ok(())
}

fn last_line(output: &Output) -> String {
    let (out, err) = text(output);
    format!("{out}{err}")
        .trim()
        .lines()
        .last()
        .unwrap_or_default()
        .to_owned()
}

fn check_desktop_and_metainfo(root: &Path, report: &mut Report) -> Result<()> {
    let desktop = root.join("snap/gui/rustidian.desktop");
    let snap_icon = "${SNAP}/meta/gui/rustidian.png";
    let content = read(&desktop)?.replace(snap_icon, "rustidian"); // o snapcraft expande isto
    let scratch = root.join(".lint-rustidian.desktop");
    fs::write(&scratch, &content)?;
    let validated = Command::new("desktop-file-validate")
        .arg(&scratch)
        .current_dir(root)
        .output();
    match validated {
        Ok(result) => {
            let (out, err) = text(&result);
            let errors: Vec<String> = out
                .lines()
                .chain(err.lines())
                .filter(|line| line.contains("error"))
                .map(str::to_owned)
                .collect();
            if !errors.is_empty() {
                report.fail("desktop-file", errors.join("; "));
            }
        }
        Err(_) => report.warn("desktop-file", "desktop-file-validate missing"),
    }
    let _ = fs::remove_file(&scratch);
    let metainfo = root.join("packaging/rustidian.metainfo.xml");
    let appstream = which("appstreamcli");
    if let Some(appstream) = &appstream {
        let result = Command::new(appstream)
            .args(["validate", "--no-net", "packaging/rustidian.metainfo.xml"])
            .current_dir(root)
            .output()?;
        if !result.status.success() {
            report.fail("appstream", last_line(&result));
        }
    }
    // 3 Nov CI: o appimagetool valida o AppStream com rigor (metainfo-filename-cid-mismatch,
    // desktop-file-not-found). A ID do componente tem de ser a mesma que o empacotador usa.
    let app_id = crate::package_appimage::APP_ID;
    let component = Regex::new(r"<id>([^<]+)</id>")?
        .captures(&read(&metainfo)?)
        .map(|capture| capture[1].to_owned())
        .unwrap_or_default();
    if component != app_id {
        report.fail(
            "appimage-metainfo",
            format!(
                "metainfo <id> {component} must equal APP_ID {app_id} (file name = component id)"
            ),
        );
    }
    let packager = read(&root.join("xtask/src/package_appimage.rs"))?;
    for (needle, why) in [
        (
            "{APP_ID}.appdata.xml",
            "metainfo file named after the component id",
        ),
        (
            "usr/share/applications/{APP_ID}.desktop",
            "desktop file in usr/share/applications named after the id",
        ),
    ] {
        if !packager.contains(needle) {
            report.fail(
                "appimage-metainfo",
                format!(
                    "package_appimage.rs must write the {why} (CI: metainfo-filename-cid-mismatch, desktop-file-not-found)"
                ),
            );
        }
    }
    if let Some(appstream) = &appstream {
        let tree = tempfile::tempdir()?;
        for folder in ["usr/share/metainfo", "usr/share/applications"] {
            fs::create_dir_all(tree.path().join(folder))?;
        }
        let info = read(&metainfo)?.replace(">rustidian.desktop<", &format!(">{app_id}.desktop<"));
        let target = tree
            .path()
            .join(format!("usr/share/metainfo/{app_id}.appdata.xml"));
        fs::write(&target, info)?;
        fs::write(
            tree.path()
                .join(format!("usr/share/applications/{app_id}.desktop")),
            read(&desktop)?.replace(snap_icon, "rustidian"),
        )?;
        let result = Command::new(appstream)
            .args(["validate", "--pedantic", "--strict", "--no-net"])
            .arg(&target)
            .current_dir(root)
            .output()?;
        if !result.status.success() {
            let (out, _) = text(&result);
            let issues: Vec<&str> = out
                .lines()
                .map(str::trim)
                .filter(|line| ["W:", "E:", "P:"].iter().any(|tag| line.starts_with(tag)))
                .collect();
            report.fail(
                "appimage-appstream",
                if issues.is_empty() {
                    out.trim().to_owned()
                } else {
                    issues.join(" | ")
                },
            );
        }
    }
    let pkgbuild = root.join("packaging/aur-bin/PKGBUILD");
    if !run(root, &bash(), &["-n", &pkgbuild.to_string_lossy()])?
        .status
        .success()
    {
        report.fail("pkgbuild-shell", "bash -n rejects PKGBUILD");
    }
    if read(&pkgbuild)?.contains("'SKIP'") {
        report.warn(
            "pkgbuild-checksum",
            "sha256sums is SKIP; fill it from the release before publishing",
        );
    }
    Ok(())
}

fn check_files_and_text(root: &Path, report: &mut Report) -> Result<()> {
    let notices = read(&root.join("THIRD_PARTY_NOTICES.txt"))?.to_lowercase();
    for phrase in [
        "SIL OPEN FONT LICENSE",
        "Ubuntu Font Licence",
        "Hack",
        "emoji-icon-font",
    ] {
        if !notices.contains(&phrase.to_lowercase()) {
            report.fail(
                "notices",
                format!("THIRD_PARTY_NOTICES.txt lacks the text for: {phrase}"),
            );
        }
    }
    for needed in ["LICENSE", "demo-vault", "assets/fonts"] {
        if !root.join(needed).exists() {
            report.fail(
                "package-input",
                format!("{needed} is missing but the package scripts copy it"),
            );
        }
    }
    let has_font_license = fs::read_dir(root.join("assets/fonts"))?
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("LICENSE-") && name.ends_with(".txt")
        });
    if !has_font_license {
        report.fail("package-input", "assets/fonts has no LICENSE-*.txt files");
    }
    for name in [
        "store-icon.png",
        "rustidian.png",
        "themes/love-space.png",
        "themes/elpis-gold.png",
        "themes/signature.png",
    ] {
        let png = root.join("docs/brand").join(name);
        let Ok(data) = fs::read(&png) else {
            report.fail(
                "brand",
                format!("{} is missing", relative_slash(root, &png)),
            );
            continue;
        };
        if data.len() < 26 {
            report.fail("brand", format!("{name} is not a PNG"));
            continue;
        }
        let width = u32::from_be_bytes(data[16..20].try_into()?);
        let height = u32::from_be_bytes(data[20..24].try_into()?);
        let color = data[25];
        if width != height || width < 256 {
            report.fail(
                "brand",
                format!("{name} must be square and at least 256 px, is {width}x{height}"),
            );
        }
        if color != 6 {
            report.fail(
                "brand",
                format!("{name} must be RGBA PNG (colour type 6), is {color}"),
            );
        }
    }
    if fs::read(root.join("snap/gui/rustidian.png"))?
        != fs::read(root.join("docs/brand/store-icon.png"))?
    {
        report.fail(
            "brand",
            "snap/gui/rustidian.png differs from docs/brand/store-icon.png; copy it again",
        );
    }
    let home = format!("/home/{}{}", "mas", "ih"); // montado em partes para este arquivo não se achar
    let grep = run(
        root,
        Path::new("git"),
        &["grep", "-l", &home, "--", ".", ":!THIRD_PARTY_NOTICES.txt"],
    )?;
    let tracked: Vec<String> = text(&grep)
        .0
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if !tracked.is_empty() {
        report.fail(
            "personal-path",
            format!(
                "tracked files name a personal home path: {}",
                tracked.join(", ")
            ),
        );
    }
    Ok(())
}

fn clippy_command(root: &Path, path: &str) -> Result<Option<String>> {
    let content = read(&root.join(path))?;
    let pattern = Regex::new(r"(?m)cargo(?: \+[\d.]+)? clippy ([^\n|]*?)(?: 2>&1)?(?:\s*\|.*)?$")?;
    Ok(pattern
        .captures(&content)
        .map(|capture| capture[1].trim().to_owned()))
}

fn check_ci_and_release_process(root: &Path, report: &mut Report) -> Result<()> {
    // 3 Nov: a checagem rápida passou e a completa reprovou no clippy de exemplos que ela nunca viu.
    let quick = clippy_command(root, ".github/workflows/native.yml")?;
    let full = clippy_command(root, ".github/workflows/ux.yml")?;
    if quick.is_none() || quick != full {
        report.fail(
            "ci-same-clippy",
            format!(
                "native.yml runs clippy '{}' but ux.yml runs '{}'; the quick check must lint what the full run lints",
                quick.unwrap_or_default(),
                full.unwrap_or_default()
            ),
        );
    }
    let ux = read(&root.join(".github/workflows/ux.yml"))?;
    if !Regex::new(r"rustidian-verified-\$\{\{ github.sha \}\}")?.is_match(&ux)
        || !ux.contains("package_after_checks")
    {
        report.fail(
            "release-artifact",
            "ux.yml no longer creates rustidian-verified-<sha> via package_after_checks; docs/RELEASING.md and the preflight are stale",
        );
    }
    let runbook = root.join("docs/RELEASING.md");
    if !runbook.is_file() {
        report.fail("release-runbook", "docs/RELEASING.md is missing");
    } else {
        let content = read(&runbook)?;
        for needle in [
            "gui_only=true",
            "package_after_checks=true",
            "publish_run",
            "cargo xtask preflight-release",
            "cargo xtask lint-packaging",
        ] {
            if !content.contains(needle) {
                report.fail(
                    "release-runbook",
                    format!("docs/RELEASING.md does not mention {needle}"),
                );
            }
        }
    }
    // 4 Nov: as notas são escritas à mão por release; o texto da 0.1.3 nunca pode sair de novo.
    if read(&root.join("xtask/src/release.rs"))?
        .contains(concat!("fixes everyday ", "vault workflows"))
    {
        report.fail(
            "release-notes",
            "xtask/src/release.rs still has the v0.1.3 release notes; rewrite them for this release",
        );
    }
    // 3 Nov: regra do dono, nenhum download pela VPN no modo caro.
    let mut networked: Vec<PathBuf> = fs::read_dir(root.join("scripts"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sh"))
        .collect();
    networked.sort();
    networked.push(root.join("xtask/src/release.rs"));
    let network = Regex::new(r"\b(gh|curl|snapcraft)\b")?;
    let cleared = Regex::new(r"unset HTTPS_PROXY|PROXY = \(|PROXY_VARS")?;
    for path in networked {
        let content = read(&path)?;
        if network.is_match(&content) && !cleared.is_match(&content) {
            report.fail(
                "no-proxy",
                format!(
                    "{} calls the network but does not clear the proxy variables",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
        }
    }
    Ok(())
}

pub fn run_all(args: &[String]) -> Result<()> {
    Args::parse(args, &[])?.only(&[])?;
    let root = root();
    let mut report = Report::default();
    type Check = fn(&Path, &mut Report) -> Result<()>;
    let checks: [(&str, Check); 5] = [
        ("check_snap", check_snap),
        ("check_workflows", check_workflows),
        ("check_desktop_and_metainfo", check_desktop_and_metainfo),
        ("check_files_and_text", check_files_and_text),
        ("check_ci_and_release_process", check_ci_and_release_process),
    ];
    for (name, check) in checks {
        if let Err(error) = check(&root, &mut report) {
            // uma checagem que quebra conta como checagem que falhou
            report.fail(name, error.to_string());
        }
    }
    for line in report.warnings.iter().chain(report.failures.iter()) {
        println!("{line}");
    }
    println!(
        "lint-packaging: {} failure(s), {} warning(s)",
        report.failures.len(),
        report.warnings.len()
    );
    if report.failures.is_empty() {
        Ok(())
    } else {
        fail("lint-packaging reprovou")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clippy_do_ci_rapido_e_do_completo_sao_lidos() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
        fs::write(
            dir.path().join(".github/workflows/a.yml"),
            "      - run: cargo +1.95.0 clippy --locked --all-targets -- -D warnings\n",
        )
        .unwrap();
        assert_eq!(
            clippy_command(dir.path(), ".github/workflows/a.yml")
                .unwrap()
                .as_deref(),
            Some("--locked --all-targets -- -D warnings")
        );
    }

    #[test]
    fn o_repositorio_passa_nas_checagens_que_nao_dependem_de_ferramentas() {
        // Roda sobre o próprio repositório: a regra de proxy, o runbook e as notas de release.
        let mut report = Report::default();
        check_ci_and_release_process(&root(), &mut report).unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }
}
