//! `cargo xtask <comando>`: empacotamento, benchmark e release do Rustidian, tudo em Rust (R8).
//! Substitui os scripts Python de `scripts/` e `tests/` (a lista está em `docs/bruno/PLANO.md`).

mod bench;
mod lint;
mod package_appimage;
mod package_deb;
mod package_linux;
mod util;

/// O mesmo arquivo do app: o instalador do pacote portátil não depende de mais nada.
#[allow(dead_code)]
#[path = "../../src/install_linux.rs"]
mod install_linux;

use util::Result;

const HELP: &str = "cargo xtask <comando> [opções]

Benchmark
  benchmark-vault                       corpus de 5000 notas + benchmark de release
  benchmark-editor --output F [--corpus repeated|diverse] [--folded]
  check-editor-timings A.json B.json    confere os limites de CPU do editor

Empacotamento (Linux)
  package-linux    --binary B --output O [--notices N]
  package-deb      --binary B --notices N --output O [--revision R]
  package-appimage --binary B --notices N --output O
  test-bundle      --binary B [--archive A]
  test-deb         --binary B --notices N [--package P] [--system]

Release
  lint-packaging                        checagens rápidas dos arquivos de empacotamento
  language-notices --baseline-notices F --output O
  preflight-release RUN_ID
  publish-release  --run RUN_ID         (só no job do Actions)
  reuse-candidate  --source-commit SHA [--check-source | --artifact-dir D --run-metadata F]";

fn dispatch(command: &str, args: &[String]) -> Result<()> {
    match command {
        "benchmark-vault" => bench::vault(args),
        "benchmark-editor" => bench::editor(args),
        "check-editor-timings" => bench::check_timings(args),
        "package-linux" => package_linux::run(args),
        "package-deb" => package_deb::run(args),
        "package-appimage" => package_appimage::run(args),
        "lint-packaging" => lint::run_all(args),
        other => util::fail(format!("Comando desconhecido: {other}\n\n{HELP}")),
    }
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        println!("{HELP}");
        return;
    };
    if command == "--help" || command == "-h" || command == "help" {
        println!("{HELP}");
        return;
    }
    let rest: Vec<String> = arguments.collect();
    if let Err(error) = dispatch(&command, &rest) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
