//! Release: `preflight-release`, `publish-release` e `reuse-candidate`
//! (antes `scripts/preflight-release.py`, `publish-checked-release.py` e `reuse-actions-candidate.py`).
//!
//! As chamadas de rede (`gh`) rodam sem as variáveis de proxy (modo caro: baixar nunca passa pela VPN).

use crate::ensure;
use crate::util::{
    Args, Result, copy_tree, fail, json_pretty, root, run_inherit, set_mode, sha256_bytes,
    sha256_file, stdout_text,
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};

const REPO: &str = "MasihMoafi/rustidian";
const PROXY_VARS: [&str; 6] = [
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
];

/// Os caminhos que precisam ser idênticos entre o commit verificado e o HEAD de quem publica:
/// o runtime e o instalador portátil (`src/install_linux.rs` já está em `src`). O metadado do
/// pacote Debian é conferido à parte. O `preflight-release` lê esta mesma lista.
pub const PUBLISH_GUARD_PATHS: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    "src",
    "vendor",
    "assets",
    "demo-vault",
    "docs/brand",
    "xtask/src/package_linux.rs",
];

fn command(program: &str, args: &[&str], root: &Path) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(root);
    for name in PROXY_VARS {
        command.env_remove(name);
    }
    command
}

fn output(command: &mut Command) -> Result<Output> {
    command.output().map_err(|e| {
        format!(
            "Não foi possível iniciar {}: {e}",
            command.get_program().to_string_lossy()
        )
        .into()
    })
}

fn ok(command: &mut Command) -> Result<bool> {
    Ok(output(command)?.status.success())
}

fn text_of(command: &mut Command) -> Result<String> {
    let result = output(command)?;
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn gh_json(root: &Path, endpoint: &str) -> Result<Value> {
    let mut gh = command("gh", &["api", endpoint], root);
    Ok(serde_json::from_str(&stdout_text(&mut gh)?)?)
}

/// Confere um release antes de despachar o job de publicação: um erro custa segundos, não uma hora de CI.
pub fn preflight(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&[])?;
    let [run_id] = args.positional.as_slice() else {
        return fail("Uso: cargo xtask preflight-release VERIFIED_RUN_ID");
    };
    ensure!(
        run_id.chars().all(|c| c.is_ascii_digit()) && !run_id.is_empty(),
        "VERIFIED_RUN_ID deve ser numérico"
    );
    let root = root();
    let mut failures: Vec<String> = Vec::new();
    let mut flag = |rule: &str, detail: String| failures.push(format!("FAIL {rule}: {detail}"));
    let _ = ok(&mut command("git", &["fetch", "-q", "origin"], &root));
    let head = text_of(&mut command("git", &["rev-parse", "HEAD"], &root))?;
    let branch = text_of(&mut command(
        "git",
        &["rev-parse", "--abbrev-ref", "HEAD"],
        &root,
    ))?;
    let info = gh_json(&root, &format!("repos/{REPO}/actions/runs/{run_id}"))?;
    let name = info["name"].as_str().unwrap_or_default();
    if name != "Rustidian UX candidate" {
        flag(
            "run-workflow",
            format!("run {run_id} is '{name}', not 'Rustidian UX candidate'"),
        );
    }
    if info["status"] != "completed" || info["conclusion"] != "success" {
        flag(
            "run-result",
            format!(
                "run {run_id} is {}/{}; publish needs a successful run",
                info["status"].as_str().unwrap_or_default(),
                info["conclusion"].as_str().unwrap_or_default()
            ),
        );
    }
    let source = info["head_sha"].as_str().unwrap_or_default().to_owned();
    let short = &source[..source.len().min(7)];
    // 4 Nov: a primeira publicação falhou, "no artifact matches rustidian-verified-<sha>": a rodada completa não o cria.
    let artifacts = gh_json(
        &root,
        &format!("repos/{REPO}/actions/runs/{run_id}/artifacts"),
    )?;
    let wanted = format!("rustidian-verified-{source}");
    let verified = artifacts["artifacts"].as_array().and_then(|list| {
        list.iter()
            .find(|artifact| artifact["name"] == wanted.as_str())
    });
    match verified {
        None => flag(
            "verified-artifact",
            format!(
                "run {run_id} has no rustidian-verified-{short} artifact. Dispatch ux.yml with -f gui_only=true -f package_after_checks=true and use THAT run id (docs/RELEASING.md)."
            ),
        ),
        Some(artifact) if artifact["expired"] == true => flag(
            "verified-artifact",
            "the verified artifact expired (1-day retention); dispatch the run again".into(),
        ),
        Some(_) => {}
    }
    // O job de publicação recusa uma árvore cujo runtime difere do commit verificado.
    if !ok(&mut command("git", &["cat-file", "-e", &source], &root))? {
        flag(
            "verified-commit",
            format!("commit {short} is not in this clone; fetch it"),
        );
    } else {
        let mut diff = vec!["diff", "--quiet", &source, "HEAD", "--"];
        diff.extend(PUBLISH_GUARD_PATHS);
        if !ok(&mut command("git", &diff, &root))? {
            flag(
                "source-drift",
                format!(
                    "{} differ between the verified {short} and HEAD {}",
                    PUBLISH_GUARD_PATHS.join(", "),
                    &head[..head.len().min(7)]
                ),
            );
        }
    }
    let remote = text_of(&mut command(
        "git",
        &["rev-parse", &format!("origin/{branch}")],
        &root,
    ))?;
    if remote != head {
        flag(
            "pushed",
            format!(
                "HEAD {} is not what origin/{branch} has; push first",
                &head[..head.len().min(7)]
            ),
        );
    }
    // O job de publicação avança `main` e falha depois de publicar se ela andou.
    if !ok(&mut command(
        "git",
        &["merge-base", "--is-ancestor", "origin/main", "HEAD"],
        &root,
    ))? {
        flag(
            "main-ancestor",
            "origin/main is not an ancestor of HEAD; cherry-pick onto the current main first"
                .into(),
        );
    }
    let cargo = fs::read_to_string(root.join("Cargo.toml"))?;
    let version = Regex::new(r#"(?m)^version = "([^"]+)""#)?
        .captures(&cargo)
        .map(|capture| capture[1].to_owned())
        .ok_or("Cargo.toml sem version")?;
    if ok(&mut command(
        "gh",
        &["release", "view", &format!("v{version}"), "--repo", REPO],
        &root,
    ))? {
        flag(
            "version",
            format!("release v{version} exists already; bump Cargo.toml and Cargo.lock"),
        );
    }
    let lock = fs::read_to_string(root.join("Cargo.lock"))?;
    let locked = Regex::new(r#"name = "rustidian"\nversion = "([^"]+)""#)?
        .captures(&lock)
        .map(|capture| capture[1].to_owned());
    if locked.as_deref() != Some(version.as_str()) {
        flag(
            "version",
            format!(
                "Cargo.lock has {} but Cargo.toml has {version}",
                locked.unwrap_or_else(|| "None".into())
            ),
        );
    }
    for file in [
        "packaging/aur-bin/PKGBUILD",
        "packaging/rustidian.metainfo.xml",
    ] {
        if !fs::read_to_string(root.join(file))?.contains(&version) {
            flag("version", format!("{file} does not mention {version}"));
        }
    }
    let lint = output(
        Command::new(std::env::current_exe()?)
            .arg("lint-packaging")
            .current_dir(&root),
    )?;
    if !lint.status.success() {
        flag(
            "lint",
            format!(
                "cargo xtask lint-packaging fails:\n{}",
                String::from_utf8_lossy(&lint.stdout).trim()
            ),
        );
    }
    for line in &failures {
        println!("{line}");
    }
    println!(
        "preflight-release: {} failure(s) for run {run_id}, HEAD {}, version {version}",
        failures.len(),
        &head[..head.len().min(7)]
    );
    if failures.is_empty() {
        Ok(())
    } else {
        fail("preflight-release reprovou")
    }
}

fn gh_command(root: &Path, args: &[&str]) -> Command {
    command("gh", args, root)
}

fn api(root: &Path, endpoint: &str, payload: Option<&Value>) -> Result<Value> {
    let mut gh = gh_command(root, &["api", endpoint]);
    if payload.is_some() {
        gh.args(["--method", "PATCH", "--input", "-"]);
    }
    gh.stdin(if payload.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = gh
        .spawn()
        .map_err(|e| format!("Não foi possível iniciar gh: {e}"))?;
    if let Some(payload) = payload {
        child
            .stdin
            .take()
            .expect("stdin em pipe")
            .write_all(payload.to_string().as_bytes())?;
    }
    let result = child.wait_with_output()?;
    ensure!(
        result.status.success(),
        "gh api {endpoint} falhou: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(serde_json::from_slice(&result.stdout)?)
}

fn release_metadata(root: &Path, tag: &str) -> Result<Value> {
    // Resolve o rascunho próprio pelo gh e lê o endpoint numérico exato: rascunhos novos
    // podem não aparecer na lista paginada da REST.
    let mut last = String::new();
    for attempt in 0..5u32 {
        let view = output(&mut gh_command(
            root,
            &["release", "view", tag, "--repo", REPO, "--json", "apiUrl"],
        ))?;
        if view.status.success() {
            let release: Value = serde_json::from_slice(&view.stdout)?;
            return api(root, release["apiUrl"].as_str().unwrap_or_default(), None);
        }
        last = String::from_utf8_lossy(&view.stderr).into_owned();
        if attempt < 4 {
            std::thread::sleep(Duration::from_secs((1u64 << (attempt + 1)).min(8)));
        }
    }
    fail(format!("gh release view {tag} falhou: {last}"))
}

fn self_command(root: &Path, args: &[&str]) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(args).current_dir(root);
    Ok(command)
}

/// Publica uma rodada UX verificada, pedida de forma explícita, num runner do Actions.
pub fn publish(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["run"])?;
    let run_id: u64 = args.require("run")?.parse()?;
    ensure!(
        std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
        "Use the explicitly dispatched Actions publication job"
    );
    let repo = std::env::var("GITHUB_REPOSITORY")?;
    ensure!(repo == REPO, "Repositório inesperado: {repo}");
    let root = root();
    let run_text = run_id.to_string();
    let run: Value = serde_json::from_str(&stdout_text(&mut gh_command(
        &root,
        &[
            "run",
            "view",
            &run_text,
            "--repo",
            &repo,
            "--json",
            "status,conclusion,headSha,workflowName,url",
        ],
    ))?)?;
    ensure!(
        run["status"] == "completed" && run["conclusion"] == "success",
        "A rodada não terminou com sucesso: {run}"
    );
    ensure!(
        run["workflowName"] == "Rustidian UX candidate",
        "Fluxo inesperado: {run}"
    );
    let source = run["headSha"].as_str().unwrap_or_default().to_owned();
    let assets = PathBuf::from("release-assets");
    run_inherit(&mut gh_command(
        &root,
        &[
            "run",
            "download",
            &run_text,
            "--repo",
            &repo,
            "--name",
            &format!("rustidian-verified-{source}"),
            "--dir",
            &assets.to_string_lossy(),
        ],
    ))?;
    for line in fs::read_to_string(assets.join("SHA256SUMS"))?.lines() {
        let (expected, name) = line
            .split_once(char::is_whitespace)
            .ok_or("SHA256SUMS malformado")?;
        let name = name.trim();
        ensure!(
            Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name),
            "Unexpected checksum path"
        );
        ensure!(
            sha256_file(&assets.join(name))? == expected,
            "Checksum diferente: {name}"
        );
    }
    let mut info: Value =
        serde_json::from_str(&fs::read_to_string(assets.join("BUILDINFO.json"))?)?;
    ensure!(
        info["source_commit"] == source.as_str(),
        "BUILDINFO de outro commit"
    );
    let version = info["version"]
        .as_str()
        .unwrap_or_default()
        .trim_start_matches("Rustidian ")
        .to_owned();
    ensure!(
        Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(&version),
        "Versão inesperada: {version}"
    );
    let checks = info["gui_checks"].as_object().cloned().unwrap_or_default();
    ensure!(
        checks.len() >= 16
            && ["table-resize", "undo", "palette-queued", "recovery"]
                .iter()
                .all(|name| checks.contains_key(*name)),
        "Faltam grupos de checagem de GUI"
    );
    ensure!(
        checks.values().all(|check| check["status"] == "passed"),
        "Há checagem de GUI que não passou"
    );
    let package = &info["debian_verification"];
    ensure!(
        package["status"] == "passed"
            && package["system_install_upgrade_remove_purge"] == true
            && package["exact_binary_sha256"] == info["binary_sha256"],
        "A verificação do pacote Debian não confere"
    );
    let deb = assets.join("rustidian-linux-amd64.deb");
    let tarball = assets.join("rustidian-linux-x86_64.tar.gz");
    ensure!(
        sha256_file(&deb)? == package["package_sha256"].as_str().unwrap_or_default(),
        "O .deb difere do verificado"
    );
    let binary_sha = info["binary_sha256"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    {
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&tarball)?));
        let mut found = false;
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.path()?.to_string_lossy() == "rustidian" {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;
                ensure!(
                    sha256_bytes(&data) == binary_sha,
                    "O executável do tar.gz difere do verificado"
                );
                found = true;
            }
        }
        ensure!(found, "O tar.gz não tem o executável");
    }
    // O runtime e o instalador portátil precisam ser iguais ao commit verificado.
    // A correção de metadados do Debian é verificada à parte, abaixo.
    let mut diff = vec!["diff", "--exit-code", source.as_str(), "HEAD", "--"];
    diff.extend(PUBLISH_GUARD_PATHS);
    run_inherit(&mut command("git", &diff, &root))?;
    // Reempacota o executável já testado com as dependências de runtime corrigidas, e
    // confere o artefato exato e o lançador instalado.
    let bundle = PathBuf::from("verified-bundle");
    {
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&tarball)?));
        archive.set_preserve_permissions(true);
        archive.unpack(&bundle)?;
    }
    let binary = bundle.join("rustidian");
    ensure!(
        sha256_file(&binary)? == binary_sha,
        "O executável extraído difere do verificado"
    );
    let notices = assets.join("THIRD_PARTY_NOTICES.txt");
    run_inherit(&mut self_command(
        &root,
        &[
            "package-deb",
            "--binary",
            &binary.to_string_lossy(),
            "--notices",
            &notices.to_string_lossy(),
            "--output",
            &deb.to_string_lossy(),
        ],
    )?)?;
    run_inherit(Command::new("sudo").args([
        "apt-get",
        "install",
        "-y",
        "--no-install-recommends",
        &format!("./{}", deb.to_string_lossy()),
    ]))?;
    ensure!(
        sha256_file(Path::new("/usr/lib/rustidian/rustidian"))? == binary_sha,
        "O executável instalado difere do verificado"
    );
    let gui_section = |section: &str| -> Result<Value> {
        run_inherit(Command::new("python3").args([
            "tests/gui_smoke.py",
            "--binary",
            "/usr/bin/rustidian",
            "--section",
            section,
        ]))?;
        let result: Value = serde_json::from_str(&fs::read_to_string("reports/gui-results.json")?)?;
        ensure!(
            result["status"] == "passed",
            "gui_smoke {section} não passou"
        );
        Ok(result)
    };
    let smoke = gui_section("undo")?;
    let palette = gui_section("palette-queued")?;
    let syntax = gui_section("syntax")?;
    let package: Value = serde_json::from_str(&stdout_text(&mut self_command(
        &root,
        &[
            "test-deb",
            "--binary",
            &binary.to_string_lossy(),
            "--package",
            &deb.to_string_lossy(),
            "--notices",
            &notices.to_string_lossy(),
            "--system",
        ],
    )?)?)?;
    ensure!(
        package["status"] == "passed" && package["exact_binary_sha256"] == binary_sha.as_str(),
        "A verificação do .deb instalado reprovou"
    );
    ensure!(
        package["package_sha256"] == sha256_file(&deb)?.as_str(),
        "O .deb mudou durante a verificação"
    );
    info["debian_verification"] = package;
    info["installed_package_gui"] = smoke;
    info["installed_package_palette"] = palette;
    info["installed_package_syntax"] = syntax;
    let publication_source = std::env::var("GITHUB_SHA")?;
    info["packaging_source_commit"] = json!(publication_source);
    info["depends"] = json!(stdout_text(
        Command::new("dpkg-deb")
            .args(["--field"])
            .arg(&deb)
            .arg("Depends")
    )?);
    fs::write(assets.join("BUILDINFO.json"), json_pretty(&info))?;
    let mut sums = String::new();
    for name in [
        "rustidian-linux-amd64.deb",
        "rustidian-linux-x86_64.tar.gz",
        "BUILDINFO.json",
    ] {
        sums.push_str(&format!("{}  {name}\n", sha256_file(&assets.join(name))?));
    }
    fs::write(assets.join("SHA256SUMS"), sums)?;
    let tag = format!("v{version}");
    let base = format!("https://github.com/{repo}/releases/download/{tag}/");
    let total: usize = checks
        .values()
        .map(|value| value["checks"].as_array().map_or(0, Vec::len))
        .sum();
    let url = run["url"].as_str().unwrap_or_default();
    let notes = assets.join("release-notes.md");
    fs::write(&notes, release_notes(&base, total, checks.len(), url))?;
    let existing = output(&mut gh_command(
        &root,
        &[
            "release",
            "view",
            &tag,
            "--repo",
            &repo,
            "--json",
            "isDraft,targetCommitish",
        ],
    ))?;
    if !existing.status.success() {
        run_inherit(&mut gh_command(
            &root,
            &[
                "release",
                "create",
                &tag,
                "--repo",
                &repo,
                "--target",
                &publication_source,
                "--draft",
                "--title",
                &format!("Rustidian {version} — Linux installers"),
                "--notes-file",
                &notes.to_string_lossy(),
            ],
        ))?;
    } else {
        let release: Value = serde_json::from_slice(&existing.stdout)?;
        ensure!(
            release["targetCommitish"] == publication_source.as_str(),
            "Existing release targets another revision"
        );
    }
    let names = [
        "rustidian-linux-amd64.deb",
        "rustidian-linux-x86_64.tar.gz",
        "BUILDINFO.json",
        "SHA256SUMS",
        "THIRD_PARTY_NOTICES.txt",
    ];
    let release = release_metadata(&root, &tag)?;
    let current = asset_digests(&release);
    for name in names {
        let expected = format!("sha256:{}", sha256_file(&assets.join(name))?);
        match current.get(name) {
            Some(digest) => ensure!(*digest == expected, "Existing asset differs: {name}"),
            None => {
                ensure!(
                    release["draft"] == true,
                    "Only an owned draft may receive new release assets"
                );
                run_inherit(&mut gh_command(
                    &root,
                    &[
                        "release",
                        "upload",
                        &tag,
                        &assets.join(name).to_string_lossy(),
                        "--repo",
                        &repo,
                    ],
                ))?;
            }
        }
    }
    let release = release_metadata(&root, &tag)?;
    let current = asset_digests(&release);
    for name in names {
        ensure!(
            current.get(name).map(String::as_str)
                == Some(format!("sha256:{}", sha256_file(&assets.join(name))?).as_str()),
            "Asset sem o digest esperado: {name}"
        );
    }
    if release["draft"] == true {
        run_inherit(&mut gh_command(
            &root,
            &[
                "release",
                "edit",
                &tag,
                "--repo",
                &repo,
                "--draft=false",
                "--latest",
                "--notes-file",
                &notes.to_string_lossy(),
            ],
        ))?;
    }
    // Uma mudança concorrente em `main` pública nunca pode ser sobrescrita.
    let main_ref = api(&root, &format!("repos/{repo}/git/ref/heads/main"), None)?["object"]["sha"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    run_inherit(&mut command(
        "git",
        &[
            "merge-base",
            "--is-ancestor",
            &main_ref,
            &publication_source,
        ],
        &root,
    ))?;
    api(
        &root,
        &format!("repos/{repo}/git/refs/heads/main"),
        Some(&json!({"sha": publication_source, "force": false})),
    )?;
    let mut asset_receipt = serde_json::Map::new();
    for name in names {
        let path = assets.join(name);
        asset_receipt.insert(
            name.to_owned(),
            json!({
                "sha256": sha256_file(&path)?,
                "bytes": fs::metadata(&path)?.len(),
                "url": format!("{base}{name}"),
            }),
        );
    }
    let receipt_url = format!("https://github.com/{repo}/releases/tag/{tag}");
    let receipt = json!({
        "status": "published",
        "tag": tag,
        "source_commit": source,
        "packaging_source_commit": publication_source,
        "verification_run": url,
        "url": receipt_url,
        "assets": Value::Object(asset_receipt),
        "native_checks": total,
        "native_groups": checks.len(),
        "network": "GitHub runner uploaded directly; no local VPN transfer",
    });
    fs::write("publication.json", json_pretty(&receipt))?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    let mut summary = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(std::env::var("GITHUB_STEP_SUMMARY")?)?;
    writeln!(
        summary,
        "[Download Rustidian .deb]({base}rustidian-linux-amd64.deb) · [Release]({receipt_url})"
    )?;
    Ok(())
}

fn asset_digests(release: &Value) -> BTreeMap<String, String> {
    release["assets"]
        .as_array()
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| {
                    Some((
                        asset["name"].as_str()?.to_owned(),
                        asset["digest"].as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// O texto das notas é escrito à mão por release (a regra `release-notes` do lint cobra isso).
fn release_notes(base: &str, total: usize, groups: usize, run_url: &str) -> String {
    format!(
        "**[Download the Debian/Ubuntu installer]({base}rustidian-linux-amd64.deb)** · [Portable Linux archive]({base}rustidian-linux-x86_64.tar.gz) · [Checksums]({base}SHA256SUMS)\n\n\
Linux x86_64, Ubuntu 24.04 or newer, glibc 2.39 or newer. Download the `.deb` and `SHA256SUMS`, then run:\n\n\
```sh\nsha256sum --check --ignore-missing SHA256SUMS\nsudo apt install ./rustidian-linux-amd64.deb\n```\n\n\
This release fixes four everyday problems. Typing `*`, `_`, a backtick, `~` or `=` over a selection now wraps it (typing `**` makes bold) instead of deleting it. Delete or Backspace over selected table body rows removes the rows. The palette is now an app-wide setting: a palette chosen in one vault applies in every vault, folder and file, where before each vault kept its own and a new one opened in teal. Opening a file or folder next to your default vault opens one window and loads only its own files. Existing notes and settings are retained; your current palette carries over.\n\n\
Verified: {total} native checks across {groups} groups; core tests; exact Debian install/upgrade/remove/purge and portable installer checks. Existing notes, recovery data and user demo edits survive. [Verification run]({run_url}). `BUILDINFO.json` binds the source and executable.\n\n\
Obsidian community plugins and browser-style HTML rendering remain unfinished. Full Obsidian parity is unfinished, and the startup comparison with Obsidian measures an earlier build only.\n"
    )
}

/// Arquivos que podem mudar sem invalidar o executável de um candidato do Actions já construído.
fn reuse_allowed(path: &str) -> bool {
    const EXACT: [&str; 11] = [
        "tests/gui_smoke.py",
        "scripts/compare-editors.py",
        "scripts/record-keyboard.py",
        "scripts/native_input.py",
        ".github/workflows/native.yml",
        ".github/workflows/ux.yml",
        "assets/icons/README.md",
        "ES.md",
        "VISION.md",
        "README.md",
        ".cargo/config.toml",
    ];
    EXACT.contains(&path) || path.starts_with("docs/") || path.starts_with("xtask/")
}

fn unchanged(root: &Path, commit: &str) -> Result<()> {
    ensure!(
        Regex::new(r"^[0-9a-f]{40}$")?.is_match(commit),
        "Expected a full source commit SHA"
    );
    run_inherit(&mut command(
        "git",
        &["merge-base", "--is-ancestor", commit, "HEAD"],
        root,
    ))?;
    let changes = stdout_text(&mut command(
        "git",
        &["diff", "--name-only", commit, "HEAD"],
        root,
    ))?;
    let rejected: Vec<&str> = changes
        .lines()
        .filter(|path| !reuse_allowed(path))
        .collect();
    ensure!(
        rejected.is_empty(),
        "Candidate reuse refused: runtime/build sources changed: {rejected:?}"
    );
    Ok(())
}

fn first_word(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned())
}

/// Restaura um release exato do Actions só quando o runtime e o build não mudaram.
pub fn reuse(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &["check-source"])?;
    args.only(&[
        "source-commit",
        "check-source",
        "artifact-dir",
        "run-metadata",
    ])?;
    let root = root();
    let commit = args.require("source-commit")?.to_owned();
    unchanged(&root, &commit)?;
    if args.flag("check-source") {
        println!("Runtime/build sources unchanged; harness-only verification is permitted.");
        return Ok(());
    }
    let (Some(artifact_dir), Some(run_metadata)) =
        (args.path("artifact-dir"), args.path("run-metadata"))
    else {
        return fail("Faltam --artifact-dir e --run-metadata");
    };
    let mut source = artifact_dir.clone();
    let raw_binary = source.join("target/release/rustidian");
    if source.join("reports").is_dir() {
        source = source.join("reports");
    }
    let run: Value = serde_json::from_str(&fs::read_to_string(&run_metadata)?)?;
    ensure!(
        run["headSha"] == commit.as_str(),
        "Source run and requested commit differ"
    );
    let marker = source.join("binary-source-commit.txt");
    let built = if marker.exists() {
        fs::read_to_string(&marker)?.trim().to_owned()
    } else {
        commit.clone()
    };
    let source_marker = source.join("source-commit.txt");
    if source_marker.exists() {
        ensure!(
            fs::read_to_string(&source_marker)?.trim() == commit,
            "source-commit.txt de outro commit"
        );
    }
    unchanged(&root, &built)?;
    let build_log_path = source.join(if raw_binary.is_file() {
        "build.log"
    } else {
        "candidate-build.log"
    });
    let build_log = fs::read_to_string(&build_log_path)?;
    ensure!(
        build_log.contains("[optimized]") && build_log.contains("release"),
        "Only optimized Actions candidates may be reused"
    );
    let reports = PathBuf::from("reports");
    copy_tree(&source, &reports)?;
    fs::write(
        reports.join("binary-source-commit.txt"),
        format!("{built}\n"),
    )?;
    fs::write(reports.join("reused-run.json"), json_pretty(&run))?;
    let output = PathBuf::from("target/release");
    let bundle = source.parent().unwrap_or(Path::new(".")).join("reused-app");
    let binary: PathBuf;
    if raw_binary.is_file() {
        binary = raw_binary;
        ensure!(
            sha256_file(&binary)? == first_word(&reports.join("binary.sha256"))?,
            "O executável difere de binary.sha256"
        );
    } else {
        let candidate = reports.join("rustidian-candidate-linux-x86_64.tar.gz");
        {
            let mut archive =
                tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&candidate)?));
            for entry in archive.entries()? {
                let entry = entry?;
                let path = entry.path()?.into_owned();
                ensure!(
                    !path.is_absolute() && !path.components().any(|part| part.as_os_str() == ".."),
                    "Caminho inseguro no candidato: {}",
                    path.display()
                );
                let kind = entry.header().entry_type();
                ensure!(
                    kind.is_file() || kind.is_dir(),
                    "Tipo de entrada inesperado: {}",
                    path.display()
                );
                let parts: Vec<String> = path
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let first = parts.first().map(String::as_str).unwrap_or_default();
                let font_license = kind.is_file()
                    && parts.len() == 3
                    && parts[0] == "assets"
                    && parts[1] == "fonts"
                    && parts[2].starts_with("LICENSE-")
                    && parts[2].ends_with(".txt");
                ensure!(
                    first == "rustidian" || first == "demo-vault" || font_license,
                    "Entrada fora do esperado no candidato: {}",
                    path.display()
                );
            }
        }
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&candidate)?));
        archive.set_preserve_permissions(true);
        archive.unpack(&bundle)?;
        binary = bundle.join("rustidian");
        ensure!(
            sha256_file(&binary)? == first_word(&reports.join("candidate.sha256"))?,
            "O executável difere de candidate.sha256"
        );
        for entry in walkdir::WalkDir::new(bundle.join("demo-vault")) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(bundle.join("demo-vault"))?;
                ensure!(
                    fs::read(entry.path())? == fs::read(Path::new("demo-vault").join(relative))?,
                    "O demo do candidato difere do repositório: {}",
                    relative.display()
                );
            }
        }
    }
    fs::create_dir_all(&output)?;
    fs::copy(&binary, output.join("rustidian"))?;
    set_mode(&output.join("rustidian"), 0o755)?;
    let benchmark = reports.join("editor-benchmark-candidate.tar.gz");
    if benchmark.exists() {
        unchanged(
            &root,
            fs::read_to_string(reports.join("benchmark-source-commit.txt"))?.trim(),
        )?;
        let directory = output.join("examples");
        fs::create_dir_all(&directory)?;
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&benchmark)?));
        let entries: Vec<_> = archive.entries()?.collect::<std::io::Result<Vec<_>>>()?;
        ensure!(
            entries.len() == 1
                && entries[0].path()?.to_string_lossy() == "editor_benchmark"
                && entries[0].header().entry_type().is_file(),
            "O candidato do benchmark deve ter só editor_benchmark"
        );
        drop(entries);
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&benchmark)?));
        archive.unpack(&directory)?;
        let bench = directory.join("editor_benchmark");
        ensure!(
            sha256_file(&bench)? == first_word(&reports.join("editor-benchmark.sha256"))?,
            "O benchmark difere de editor-benchmark.sha256"
        );
        set_mode(&bench, 0o755)?;
    } else {
        for name in [
            "benchmark-source-commit.txt",
            "benchmark-toolchain.txt",
            "editor-benchmark.sha256",
        ] {
            let _ = fs::remove_file(reports.join(name));
        }
    }
    println!(
        "Restored exact optimized binary from {built}; runtime sources and sample bytes match."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuso_so_aceita_arquivos_que_nao_mudam_o_executavel() {
        assert!(reuse_allowed("docs/RELEASING.md"));
        assert!(reuse_allowed("xtask/src/release.rs"));
        assert!(reuse_allowed(".github/workflows/ux.yml"));
        assert!(!reuse_allowed("src/app.rs"));
        assert!(!reuse_allowed("Cargo.lock"));
    }

    #[test]
    fn lista_de_guarda_cobre_o_runtime() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "src",
            "vendor",
            "assets",
            "demo-vault",
        ] {
            assert!(PUBLISH_GUARD_PATHS.contains(&path), "{path}");
        }
    }
}
