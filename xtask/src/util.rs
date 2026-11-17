//! Peças comuns: erro simples, argumentos, hash, processos e o JSON no formato do Python.

use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Falha com uma mensagem (o equivalente do `SystemExit("...")` do Python).
pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(message.into().into())
}

/// `assert` com mensagem: o `assert cond, msg` dos scripts antigos.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            return $crate::util::fail(format!($($arg)+));
        }
    };
}

/// A raiz do repositório (a pasta acima de `xtask/`).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask fica dentro do repositório")
        .to_path_buf()
}

/// A pasta de build: `CARGO_TARGET_DIR` se existir, senão `target/` da raiz.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"))
}

pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Escreve `<saída>.sha256` como `sha256sum`: `<hash>  <nome>`.
pub fn write_sha256_sidecar(output: &Path) -> Result<()> {
    let name = output
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    let mut sidecar = output.as_os_str().to_owned();
    sidecar.push(".sha256");
    fs::write(sidecar, format!("{}  {name}\n", sha256_file(output)?))?;
    Ok(())
}

/// Argumentos `--nome valor`, `--nome` (marcador) e posicionais, sem dependência externa.
pub struct Args {
    values: HashMap<String, String>,
    flags: HashSet<String>,
    pub positional: Vec<String>,
}

impl Args {
    /// `flags` são as opções sem valor; todas as outras `--x` esperam um valor.
    pub fn parse(raw: &[String], flags: &[&str]) -> Result<Self> {
        let mut args = Args {
            values: HashMap::new(),
            flags: HashSet::new(),
            positional: Vec::new(),
        };
        let mut iter = raw.iter();
        while let Some(item) = iter.next() {
            if let Some(name) = item.strip_prefix("--") {
                if flags.contains(&name) {
                    args.flags.insert(name.to_owned());
                } else if let Some((name, value)) = name.split_once('=') {
                    args.values.insert(name.to_owned(), value.to_owned());
                } else {
                    let Some(value) = iter.next() else {
                        return fail(format!("A opção --{name} precisa de um valor"));
                    };
                    args.values.insert(name.to_owned(), value.clone());
                }
            } else {
                args.positional.push(item.clone());
            }
        }
        Ok(args)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    pub fn opt(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.opt(name).map(PathBuf::from)
    }

    pub fn require(&self, name: &str) -> Result<&str> {
        self.opt(name)
            .ok_or_else(|| format!("Falta a opção --{name}").into())
    }

    pub fn require_path(&self, name: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.require(name)?))
    }

    /// Recusa opções que o comando não conhece (o `argparse` fazia o mesmo).
    pub fn only(&self, known: &[&str]) -> Result<()> {
        for name in self.values.keys().chain(self.flags.iter()) {
            if !known.contains(&name.as_str()) {
                return fail(format!("Opção desconhecida: --{name}"));
            }
        }
        Ok(())
    }
}

fn describe(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

/// Roda e exige sucesso; devolve stdout e stderr.
pub fn run_checked(command: &mut Command) -> Result<Output> {
    let output = command
        .output()
        .map_err(|e| format!("Não foi possível iniciar {}: {e}", describe(command)))?;
    if !output.status.success() {
        return fail(format!(
            "{} falhou ({}):\n{}{}",
            describe(command),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
}

pub fn stdout_text(command: &mut Command) -> Result<String> {
    let output = run_checked(command)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Roda um comando que vai ao terminal, herdando a saída, e exige sucesso.
pub fn run_inherit(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|e| format!("Não foi possível iniciar {}: {e}", describe(command)))?;
    if !status.success() {
        return fail(format!("{} terminou com {status}", describe(command)));
    }
    Ok(())
}

/// Roda com prazo; ao estourar, mata o processo.
pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Não foi possível iniciar {}: {e}", describe(command)))?;
    let mut stdout = child.stdout.take().expect("stdout em pipe");
    let mut stderr = child.stderr.take().expect("stderr em pipe");
    let out = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let err = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return fail(format!(
                "{} passou de {}s e foi encerrado",
                describe(command),
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// O `shutil.which`: procura um programa no PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|folder| folder.join(exe(name)))
        .find(|candidate| candidate.is_file())
}

/// O `bash` real: no Windows o `bash.exe` de System32 é o do WSL e não serve; prefere o do Git.
pub fn bash() -> PathBuf {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&paths)
        .map(|folder| folder.join(exe("bash")))
        .find(|candidate| {
            candidate.is_file()
                && !candidate
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("system32")
        });
    found
        .or_else(|| {
            [
                "C:/Program Files/Git/bin/bash.exe",
                "C:/Program Files/Git/usr/bin/bash.exe",
            ]
            .iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("bash"))
}

/// Copia uma pasta inteira, criando o destino.
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(from)?;
        let target = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// `chmod` onde existe; no Windows não faz nada (o pacote é Linux).
pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

/// O caminho relativo à raiz, com `/`, como os scripts antigos gravavam.
pub fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// O `json.dumps` do Python numa linha só: separadores `, ` e `: `.
pub fn json_line(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(json_line).collect::<Vec<_>>().join(", ")
            )
        }
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, item)| format!("{}: {}", Value::String(key.clone()), json_line(item)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

/// O `json.dumps(indent=2)` mais a quebra de linha final que os scripts gravavam.
pub fn json_pretty(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("JSON serializável");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_line_usa_os_separadores_do_python() {
        let value = json!({"report": "a.json", "bytes": 3, "idle_us": 1.5, "list": [1, 2]});
        assert_eq!(
            json_line(&value),
            r#"{"report": "a.json", "bytes": 3, "idle_us": 1.5, "list": [1, 2]}"#
        );
    }

    #[test]
    fn args_aceita_valor_marcador_e_igual() {
        let raw: Vec<String> = ["--output", "x", "--folded", "--corpus=diverse", "p"]
            .map(String::from)
            .to_vec();
        let args = Args::parse(&raw, &["folded"]).unwrap();
        assert_eq!(args.opt("output"), Some("x"));
        assert_eq!(args.opt("corpus"), Some("diverse"));
        assert!(args.flag("folded"));
        assert_eq!(args.positional, ["p"]);
        assert!(args.only(&["output", "folded", "corpus"]).is_ok());
        assert!(args.only(&["output"]).is_err());
    }

    #[test]
    fn sha256_conhecido() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
