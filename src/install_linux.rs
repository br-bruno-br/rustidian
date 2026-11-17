//! Instalador do pacote portátil do Linux (`rustidian --install`), sem root e sem Python.
//!
//! Só usa a biblioteca padrão: o `xtask` inclui este arquivo por `#[path]` para reaproveitar
//! `MIME_TYPES` e o texto do lançador sem depender do app inteiro.

use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const MIME_TYPES: &str = "text/markdown;text/x-markdown;text/plain;text/rust;\
text/x-c;text/x-csrc;text/x-chdr;text/x-c++;text/x-c++src;text/x-c++hdr;\
text/x-python;text/x-python3;text/javascript;application/javascript;\
text/x-java;text/x-go;application/x-ruby;application/x-shellscript;\
text/x-shellscript;application/json;application/toml;application/yaml;\
text/yaml;application/xml;text/xml;text/css;text/html;application/sql;";

/// Um lançamento pelo dock não tem as pastas que um shell interativo põe no PATH, então a
/// aba do agente não acharia agentes instalados nelas. Prefixar na ordem inversa mantém a ordem do shell.
pub const LAUNCHER_PATH: &str = "for folder in /usr/local/node/bin \"$HOME/.local/bin\"; do\n\
    case \":$PATH:\" in\n\
        *\":$folder:\"*) ;;\n\
        *) if [ -d \"$folder\" ]; then PATH=\"$folder${PATH:+:$PATH}\"; fi ;;\n\
    esac\n\
done\n\
export PATH\n";

/// O `shlex.quote` do Python: aspas simples só quando preciso.
pub fn shell_quote(value: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c);
    if !value.is_empty() && value.chars().all(safe) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn desktop_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// O Desktop Entry tem duas camadas de escape e trata `%` como código de campo.
pub fn desktop_exec(value: &str) -> String {
    let mut value = value.replace('%', "%%");
    for character in ['\\', '"', '`', '$'] {
        value = value.replace(character, &format!("\\{character}"));
    }
    desktop_string(&format!("\"{value}\""))
}

/// O script `bin/rustidian` que o instalador grava.
pub fn launcher_script(installed_binary: &Path, default_vault: Option<&Path>) -> String {
    let mut script = String::from("#!/bin/sh\n");
    script.push_str(LAUNCHER_PATH);
    if let Some(vault) = default_vault {
        script.push_str(&format!(
            "if [ -z \"${{RUSTIDIAN_DEFAULT_VAULT+x}}\" ]; then\n    export RUSTIDIAN_DEFAULT_VAULT={}\nfi\n",
            shell_quote(&vault.to_string_lossy())
        ));
    }
    script.push_str(&format!(
        "exec {} \"$@\"\n",
        shell_quote(&installed_binary.to_string_lossy())
    ));
    script
}

/// O arquivo `.desktop` do menu de aplicativos.
pub fn desktop_entry(launcher: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Rustidian\n\
Comment=Write and connect local Markdown notes\n\
Exec={} %f\n\
Icon=rustidian\nTerminal=false\n\
Categories=Office;TextEditor;\nKeywords=Markdown;Notes;Vault;\n\
StartupWMClass=rustidian\nStartupNotify=true\nMimeType={MIME_TYPES}\n",
        desktop_exec(&launcher.to_string_lossy())
    )
}

/// Copia por um arquivo temporário na mesma pasta e troca de uma vez, com o modo pedido.
pub fn atomic_write(contents: &[u8], destination: &Path, mode: u32) -> Result<(), String> {
    let folder = destination
        .parent()
        .ok_or_else(|| format!("Sem pasta de destino: {}", destination.display()))?;
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut name = OsString::from(".instalando-");
    name.push(destination.file_name().unwrap_or_default());
    let temporary = folder.join(name);
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        set_mode(&temporary, mode)?;
        fs::rename(&temporary, destination)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&temporary);
        format!("{}: {e}", destination.display())
    })
}

fn atomic_copy(source: &Path, destination: &Path, mode: u32) -> Result<(), String> {
    let contents = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    atomic_write(&contents, destination, mode)
}

fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

pub struct Options {
    /// `--prefix`: raiz da instalação (padrão `~/.local`).
    pub prefix: Option<PathBuf>,
    /// `--default-vault`: pasta que abre quando nenhum arquivo é passado.
    pub default_vault: Option<PathBuf>,
}

impl Options {
    /// Lê `--prefix X` e `--default-vault X` (também `--opção=X`).
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Options {
            prefix: None,
            default_vault: None,
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let (name, inline) = match argument.split_once('=') {
                Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
                None => (argument, None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| arguments.next())
                    .map(PathBuf::from)
                    .ok_or_else(|| format!("{name} precisa de um valor"))
            };
            match name.as_str() {
                "--prefix" => options.prefix = Some(value()?),
                "--default-vault" => options.default_vault = Some(value()?),
                other => return Err(format!("Opção desconhecida para --install: {other}")),
            }
        }
        Ok(options)
    }
}

/// O que `--install` imprime ao terminar.
pub struct Installed {
    pub launcher: PathBuf,
}

/// Instala o pacote extraído em `bundle` (a pasta do executável). Valida tudo antes de mexer
/// numa instalação existente; `version` é a linha de `--version`.
pub fn install(bundle: &Path, options: &Options, version: &str) -> Result<Installed, String> {
    for name in ["rustidian", "rustidian.png", "demo-vault", "LICENSE"] {
        if !bundle.join(name).exists() {
            return Err(format!("Incomplete bundle: missing {name}"));
        }
    }
    if let Some(vault) = &options.default_vault
        && !vault.is_dir()
    {
        return Err("--default-vault must be an existing folder".into());
    }
    if !version.starts_with("Rustidian ") {
        return Err("This bundle does not contain a Rustidian executable".into());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let prefix = match &options.prefix {
        Some(prefix) => absolute(prefix),
        None => absolute(
            &home
                .ok_or("Sem pasta pessoal (HOME) para instalar")?
                .join(".local"),
        ),
    };
    let data = if options.prefix.is_some() {
        prefix.join("share")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| prefix.join("share"))
    };
    let target = prefix.join("lib/rustidian");
    let launcher = prefix.join("bin/rustidian");
    atomic_copy(&bundle.join("rustidian"), &target.join("rustidian"), 0o755)?;
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.txt"] {
        if bundle.join(name).is_file() {
            atomic_copy(&bundle.join(name), &target.join(name), 0o644)?;
        }
    }
    // As edições do demo pertencem ao usuário depois da primeira instalação.
    if !target.join("demo-vault").exists() {
        copy_folder(&bundle.join("demo-vault"), &target.join("demo-vault"))?;
    }
    let vault = options.default_vault.as_deref().map(absolute);
    let script = launcher_script(&target.join("rustidian"), vault.as_deref());
    atomic_write(script.as_bytes(), &launcher, 0o755)?;
    let icon = data.join("icons/hicolor/256x256/apps/rustidian.png");
    atomic_copy(&bundle.join("rustidian.png"), &icon, 0o644)?;
    touch_folder(&data.join("icons/hicolor")); // os temas de ícones reexaminam pela data da pasta
    let applications = data.join("applications");
    atomic_write(
        desktop_entry(&launcher).as_bytes(),
        &applications.join("rustidian.desktop"),
        0o644,
    )?;
    if let Some(update) = find_in_path("update-desktop-database") {
        let status = std::process::Command::new(update)
            .arg(&applications)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("update-desktop-database terminou com {status}"));
        }
    }
    Ok(Installed { launcher })
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn touch_folder(folder: &Path) {
    #[cfg(unix)]
    if let Ok(file) = fs::File::open(folder) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
    #[cfg(not(unix))]
    let _ = folder;
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|folder| folder.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_acompanha_o_shlex() {
        assert_eq!(shell_quote("/usr/bin/x-1.2"), "/usr/bin/x-1.2");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(shell_quote("$HOME `x`"), "'$HOME `x`'");
    }

    #[test]
    fn desktop_exec_escapa_as_duas_camadas() {
        assert_eq!(desktop_exec("/a b/c"), "\"/a b/c\"");
        assert_eq!(desktop_exec("/p/100%"), "\"/p/100%%\"");
        // Cada um dos quatro caracteres ganha uma barra; o `\` passa pela segunda camada e dobra.
        assert_eq!(desktop_exec("a\"b"), "\"a\\\\\"b\"");
        assert_eq!(desktop_exec("a$b"), "\"a\\\\$b\"");
    }

    #[test]
    fn lancador_poe_o_cofre_padrao_so_se_nao_definido() {
        let script = launcher_script(
            Path::new("/p/lib/rustidian/rustidian"),
            Some(Path::new("/v/Vault x")),
        );
        assert!(script.starts_with("#!/bin/sh\nfor folder in /usr/local/node/bin"));
        assert!(script.contains("if [ -z \"${RUSTIDIAN_DEFAULT_VAULT+x}\" ]; then"));
        assert!(script.contains("export RUSTIDIAN_DEFAULT_VAULT='/v/Vault x'"));
        assert!(script.ends_with("exec /p/lib/rustidian/rustidian \"$@\"\n"));
        let sem = launcher_script(Path::new("/p/rustidian"), None);
        assert!(!sem.contains("RUSTIDIAN_DEFAULT_VAULT"));
    }

    #[test]
    fn entrada_do_menu_lista_os_tipos() {
        let entry = desktop_entry(Path::new("/p/bin/rustidian"));
        assert!(entry.contains("Exec=\"/p/bin/rustidian\" %f\n"));
        assert!(entry.contains("MimeType=text/markdown;"));
        assert!(entry.ends_with("application/sql;\n"));
    }

    #[test]
    fn opcoes_leem_prefixo_e_cofre() {
        let options =
            Options::parse(["--prefix", "/p", "--default-vault=/v"].map(String::from)).unwrap();
        assert_eq!(options.prefix.as_deref(), Some(Path::new("/p")));
        assert_eq!(options.default_vault.as_deref(), Some(Path::new("/v")));
        assert!(Options::parse(["--x".to_owned()]).is_err());
        assert!(Options::parse(["--prefix".to_owned()]).is_err());
    }

    /// O que `tests/launcher_path.py` conferia: a ordem do PATH do lançador, rodando de verdade.
    #[cfg(unix)]
    #[test]
    fn lancador_poe_as_pastas_na_ordem_do_shell() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let app = base.path().join("app");
        fs::write(&app, "#!/bin/sh\nprintf %s \"$PATH\"\n").unwrap();
        fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
        let launcher = base.path().join("rustidian");
        atomic_write(launcher_script(&app, None).as_bytes(), &launcher, 0o755).unwrap();
        let dock = "/usr/local/bin:/usr/bin:/bin";
        let run = |home: &Path, path: &str| {
            let output = std::process::Command::new(&launcher)
                .env_clear()
                .env("HOME", home)
                .env("PATH", path)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap()
        };
        let home = base.path().join("home");
        let local_bin = home.join(".local/bin");
        fs::create_dir_all(&local_bin).unwrap();
        let node = Path::new("/usr/local/node/bin");
        let node: Vec<String> = node
            .is_dir()
            .then(|| node.to_string_lossy().into_owned())
            .into_iter()
            .collect();
        let local = local_bin.to_string_lossy().into_owned();
        let join = |parts: &[&str]| parts.join(":");
        let mut esperado = vec![local.as_str()];
        esperado.extend(node.iter().map(String::as_str));
        esperado.push(dock);
        assert_eq!(run(&home, dock), join(&esperado));
        // Pastas que já estão no PATH ficam onde estão, uma vez.
        let presente = format!("{dock}:{local}:/usr/local/node/bin");
        assert_eq!(run(&home, &presente), presente);
        // Uma pasta que não existe não entra.
        let mut sem_home = node.iter().map(String::as_str).collect::<Vec<_>>();
        sem_home.push(dock);
        assert_eq!(run(&base.path().join("sem home"), dock), join(&sem_home));
    }
}
