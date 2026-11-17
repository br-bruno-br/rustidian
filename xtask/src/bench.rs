//! Benchmarks: `benchmark-vault`, `benchmark-editor` e `check-editor-timings`
//! (antes `scripts/benchmark-vault.py`, `benchmark-editor.py` e `check-editor-timings.py`).

use crate::ensure;
use crate::util::{
    Args, Result, exe, fail, json_line, json_pretty, relative_slash, root, run_with_timeout,
    sha256_file, stdout_text, target_dir,
};
use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::Duration};

/// Gera o corpus determinístico (5000 notas) e roda o benchmark de release.
pub fn vault(args: &[String]) -> Result<()> {
    Args::parse(args, &[])?.only(&[])?;
    let root = root();
    let binary = target_dir()
        .join("release/examples")
        .join(exe("vault_benchmark"));
    ensure!(
        binary.is_file(),
        "Falta {}; rode antes: cargo build --release --example vault_benchmark",
        binary.display()
    );
    let corpus = tempfile::Builder::new()
        .prefix("rustidian-corpus-")
        .tempdir()?;
    let mut total_bytes = 0usize;
    for number in 0..5000 {
        let name = if number == 0 {
            "Home.md".to_owned()
        } else {
            format!("Note {number:05}.md")
        };
        let text = format!("# Note {number:05}\n\n")
            + &"Connected notes, 你好, **Markdown** and [[Home]]. Search and navigate this vault.\n\n"
                .repeat(16);
        fs::write(corpus.path().join(name), &text)?;
        total_bytes += text.len();
    }
    let completed = stdout_text(Command::new(&binary).arg(corpus.path()))?;
    let samples: Value = serde_json::from_str(&completed)?;
    let result = json!({
        "corpus": {
            "notes": 5000,
            "utf8_bytes": total_bytes,
            "recipe": "cargo xtask benchmark-vault; sequential names, 16 identical paragraphs per note",
        },
        "samples": samples,
        "build": {
            "binary_sha256": sha256_file(&binary)?,
            "rustc": stdout_text(Command::new("rustc").arg("--version"))?,
        },
    });
    fs::create_dir_all(root.join("reports"))?;
    fs::write(
        root.join("reports/vault-benchmark.json"),
        json_pretty(&result),
    )?;
    println!("{completed}");
    Ok(())
}

fn sorted_files(root: &Path, folder: &str, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    let mut found: Vec<String> = walkdir::WalkDir::new(root.join(folder))
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file() && keep(entry.path()))
        .map(|entry| relative_slash(root, entry.path()))
        .collect();
    found.sort();
    found
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false)
}

/// A média de carga do sistema (só no Linux).
fn load_average() -> Value {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|text| {
            Value::Array(
                text.split_whitespace()
                    .take(3)
                    .filter_map(|part| part.parse::<f64>().ok())
                    .map(Value::from)
                    .collect(),
            )
        })
        .unwrap_or(Value::Null)
}

fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split_once(':'))
                .map(|(_, name)| name.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn platform() -> String {
    let release = fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    format!(
        "{}-{release}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Roda o experimento do editor nativo e grava o relatório com a proveniência.
pub fn editor(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &["folded"])?;
    args.only(&["output", "corpus", "folded"])?;
    let output = args.require_path("output")?;
    let diverse = match args.opt("corpus").unwrap_or("repeated") {
        "repeated" => false,
        "diverse" => true,
        other => {
            return fail(format!(
                "--corpus deve ser repeated ou diverse, não {other}"
            ));
        }
    };
    let root = root();
    let binary = target_dir()
        .join("release/examples")
        .join(exe("editor_benchmark"));
    ensure!(
        binary.is_file(),
        "Falta {}; rode antes: cargo build --release --example editor_benchmark",
        binary.display()
    );
    fs::create_dir_all(root.join("reports"))?;
    let lock = fs::File::create(root.join("reports/.gui-smoke.lock"))?;
    if lock.try_lock().is_err() {
        return fail("A native GUI run is active; finish it before measuring.");
    }
    ensure!(
        std::env::var_os("RUSTIDIAN_UI_PROBE").is_none(),
        "RUSTIDIAN_UI_PROBE está definido; a medição precisa do app sem sonda"
    );
    let mut paths = vec![
        "examples/editor_benchmark.rs".to_owned(),
        "Cargo.toml".to_owned(),
        "Cargo.lock".to_owned(),
    ];
    let is_rs = |path: &Path| path.extension().is_some_and(|ext| ext == "rs");
    paths.extend(sorted_files(&root, "src", is_rs));
    paths.extend(sorted_files(&root, "vendor", is_rs));
    paths.extend(sorted_files(&root, "vendor", |path| {
        path.file_name().is_some_and(|name| name == "Cargo.toml")
    }));
    let fonts = fs::read_dir(root.join("assets/fonts"))?;
    let mut font_paths: Vec<String> = fonts
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "ttf" || ext == "otf")
        })
        .map(|path| relative_slash(&root, &path))
        .collect();
    font_paths.sort();
    paths.extend(font_paths);
    let mut sources = serde_json::Map::new();
    for path in &paths {
        sources.insert(path.clone(), Value::String(sha256_file(&root.join(path))?));
    }
    let binary_sha256 = sha256_file(&binary)?;
    let mut provenance = json!({
        "started_utc": utc_now(),
        "platform": platform(),
        "cpu": cpu_model(),
        "rustc": stdout_text(Command::new("rustc").arg("--version"))?,
        "binary_sha256": binary_sha256,
        "source_sha256": Value::Object(sources.clone()),
        "load_average_before": load_average(),
    });
    let mut command = Command::new(&binary);
    if diverse {
        command.arg("--diverse");
    }
    if args.flag("folded") {
        command.arg("--folded");
    }
    command.current_dir(&root);
    let result = run_with_timeout(&mut command, Duration::from_secs(120))?;
    fs::write(output.with_extension("log"), &result.stderr)?;
    ensure!(
        result.status.success(),
        "O benchmark do editor terminou com {}",
        result.status
    );
    let mut report: Value = serde_json::from_slice(&result.stdout)?;
    ensure!(
        sha256_file(&binary)? == binary_sha256,
        "O executável mudou durante a medição"
    );
    for (path, digest) in &sources {
        ensure!(
            sha256_file(&root.join(path))? == digest.as_str().unwrap_or_default(),
            "{path} mudou durante a medição"
        );
    }
    provenance["finished_utc"] = json!(utc_now());
    provenance["load_average_after"] = load_average();
    report["provenance"] = provenance;
    if let Some(folder) = output.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(&output, json_pretty(&report))?;
    let conditions = report["conditions"].as_array().map_or(0, Vec::len);
    println!(
        "Recorded {conditions} native editor conditions in {}",
        output.display()
    );
    Ok(())
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    }
}

fn phase_median(condition: &Value, phase: &str) -> f64 {
    median(
        condition[phase]
            .as_array()
            .map(|samples| {
                samples
                    .iter()
                    .filter_map(|sample| sample["ui_us"].as_f64())
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// Confere um relatório contra os limites de CPU declarados.
pub fn check_report(path: &Path) -> Result<()> {
    let report: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let name = path.display();
    let conditions = report["conditions"].as_array().cloned().unwrap_or_default();
    let blocks: Vec<i64> = conditions
        .iter()
        .filter_map(|condition| condition["blocks"].as_i64())
        .collect();
    ensure!(blocks == [20, 200, 500], "{name}: incomplete corpus");
    for condition in &conditions {
        ensure!(
            condition["insertions_verified"] == 25,
            "{name}: input preservation failed"
        );
        let complete = ["stable", "edited"].iter().all(|phase| {
            condition[*phase]
                .as_array()
                .is_some_and(|samples| samples.len() == 20)
        });
        ensure!(complete, "{name}: incomplete samples");
        let idle = phase_median(condition, "stable");
        let edited = phase_median(condition, "edited");
        println!(
            "{}",
            json_line(&json!({
                "report": name.to_string(),
                "bytes": condition["source_bytes"],
                "idle_us": idle,
                "edited_us": edited,
            }))
        );
        ensure!(
            !(idle > 5000.0 || edited > 16700.0),
            "{name}: editor CPU limit exceeded"
        );
    }
    Ok(())
}

pub fn check_timings(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&[])?;
    if args.positional.len() != 2 {
        return fail("Pass unfolded and folded raw JSON reports");
    }
    for path in &args.positional {
        check_report(Path::new(path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(blocks: i64, idle: f64, edited: f64) -> Value {
        let samples = |us: f64| -> Vec<Value> { (0..20).map(|_| json!({"ui_us": us})).collect() };
        json!({"blocks": blocks, "insertions_verified": 25, "source_bytes": blocks * 100,
               "stable": samples(idle), "edited": samples(edited)})
    }

    fn write(dir: &Path, name: &str, conditions: Vec<Value>) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, json!({"conditions": conditions}).to_string()).unwrap();
        path
    }

    #[test]
    fn mediana_de_vinte_amostras_e_a_media_dos_dois_do_meio() {
        let values: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(median(values), 10.5);
    }

    #[test]
    fn relatorio_dentro_dos_limites_passa() {
        let dir = tempfile::tempdir().unwrap();
        let good = write(
            dir.path(),
            "ok.json",
            vec![
                condition(20, 100.0, 200.0),
                condition(200, 500.0, 900.0),
                condition(500, 5000.0, 16700.0),
            ],
        );
        assert!(check_report(&good).is_ok());
    }

    #[test]
    fn relatorio_acima_do_limite_ou_incompleto_falha() {
        let dir = tempfile::tempdir().unwrap();
        let slow = write(
            dir.path(),
            "slow.json",
            vec![
                condition(20, 100.0, 200.0),
                condition(200, 5001.0, 900.0),
                condition(500, 1.0, 1.0),
            ],
        );
        assert!(
            check_report(&slow)
                .unwrap_err()
                .to_string()
                .contains("CPU limit")
        );
        let short = write(dir.path(), "short.json", vec![condition(20, 1.0, 1.0)]);
        assert!(
            check_report(&short)
                .unwrap_err()
                .to_string()
                .contains("incomplete corpus")
        );
    }
}
