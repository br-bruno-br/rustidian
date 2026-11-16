use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use std::path::{Component, Path, PathBuf};

/// Read one small, ordinary settings file; never follow a linked profile.
pub fn read_obsidian<T: serde::de::DeserializeOwned + Default>(
    root: &Path,
    name: &str,
) -> Result<T, String> {
    let folder = root.join(".obsidian");
    match std::fs::symlink_metadata(&folder) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        _ => {
            return Err(
                "Cannot import note settings from a linked or unreadable .obsidian folder.".into(),
            );
        }
    }
    let path = folder.join(name);
    let meta = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(format!("Cannot read note settings: {error}")),
        Ok(meta) => meta,
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return Err("Note settings must be small ordinary JSON files.".into());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("Cannot read note settings: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("Cannot import note settings: {error}"))
}

/// The user's home folder: `HOME`, or `USERPROFILE` on Windows.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| {
            cfg!(windows)
                .then(|| std::env::var_os("USERPROFILE"))
                .flatten()
        })
        .map(PathBuf::from)
}

/// Where the app keeps per-user state (sessions, recovery history, the agent's context file):
/// `XDG_STATE_HOME` or `~/.local/state`, and `%LOCALAPPDATA%` on Windows.
pub fn state_base() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    }
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/state")))
}

/// Where the app keeps per-user data (the vault list): `XDG_DATA_HOME` or `~/.local/share`,
/// and `%LOCALAPPDATA%` on Windows.
pub fn data_base() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
}

pub fn state_file(root: &Path, group: &str, extension: &str) -> Option<PathBuf> {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let base = state_base()?;
    let mut hash = DefaultHasher::new();
    root.hash(&mut hash);
    Some(
        app_directory(&base)
            .join(group)
            .join(format!("{:016x}.{extension}", hash.finish())),
    )
}

/// Keep settings and recovery when upgrading, including windows still running.
pub fn app_directory(base: &Path) -> PathBuf {
    let current = base.join("rustidian");
    let previous = base.join("rust-notes");
    if current.exists() || !previous.is_dir() {
        return current;
    }
    if let Err(error) = std::fs::rename(&previous, &current) {
        eprintln!(
            "Cannot move existing Rustidian settings: {error}. Using their previous location."
        );
        return previous;
    }
    #[cfg(unix)]
    if let Err(error) = std::os::unix::fs::symlink("rustidian", &previous) {
        eprintln!("Cannot keep the settings link for an older running window: {error}");
    }
    current
}

/// A relative path as it is written in links and searched: components joined with `/` on every
/// system. On Windows the separator is `\`, which a wiki link cannot hold; on Linux a `\` is an
/// ordinary character of a name and stays.
pub fn slashed(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

pub fn wiki_destination(path: &Path) -> String {
    if crate::vault::is_markdown(path) {
        let stem = path.with_extension("");
        if crate::assets::is_image(&stem) {
            slashed(path)
        } else {
            slashed(&stem)
        }
    } else {
        slashed(path)
            .replace('%', "%25")
            .replace('#', "%23")
            .replace('|', "%7C")
            .replace('\\', "%5C")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
            .replace("]]", "%5D%5D")
            .replace("mailto:", "mailto%3A")
    }
}

pub fn display_path(path: &Path) -> String {
    if crate::vault::is_markdown(path) {
        slashed(&path.with_extension(""))
    } else {
        slashed(path)
    }
}

pub fn wiki_path_supported(path: &Path) -> bool {
    if path.to_str().is_none() {
        return false;
    }
    if !crate::vault::is_markdown(path) {
        return true;
    }
    let raw = slashed(path);
    let name = &raw[..raw.len() - 3];
    !name.is_empty()
        && name.trim() == name
        && !name.contains("]]")
        && !name
            .chars()
            .any(|character| matches!(character, '#' | '|' | '\\' | '\n' | '\r'))
}

pub fn moved_path(path: &Path, from: &Path, to: &Path, folder: bool) -> PathBuf {
    if path == from {
        to.to_owned()
    } else if folder && let Ok(suffix) = path.strip_prefix(from) {
        to.join(suffix)
    } else {
        path.to_owned()
    }
}

pub fn normalise(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(result)
}

pub fn markdown_name(path: &Path) -> PathBuf {
    if crate::vault::is_markdown(path) {
        return path.to_owned();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".md");
    PathBuf::from(name)
}

pub fn decode(value: &str) -> Option<String> {
    percent_decode_str(value)
        .decode_utf8()
        .ok()
        .map(|value| value.into_owned())
}

/// Characters a path may not carry unescaped inside a link or a `file://` URI.
const RESERVED: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'(')
    .add(b')')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'\\');

/// The `file://` URI of an absolute path on this system. On Windows `canonicalize` yields the
/// verbatim form `\\?\C:\dir\file`, and the URI must be `file:///C:/dir/file`, never the
/// `file:////%3F/C:/...` that treating the prefix as a folder produced.
pub fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        return windows_file_uri(&text);
    }
    format!("file://{}", utf8_percent_encode(&text, RESERVED))
}

/// `file_uri` for a Windows path written as text (`C:\x`, `\\?\C:\x`, `\\?\UNC\srv\share\x`,
/// `\\srv\share\x`). Pure, so it is tested on every system.
pub fn windows_file_uri(text: &str) -> String {
    let slashed = text.replace('\\', "/");
    let rest = slashed
        .strip_prefix("//?/")
        .or_else(|| slashed.strip_prefix("//./"))
        .unwrap_or(&slashed);
    let encoded = |value: &str| utf8_percent_encode(value, RESERVED).to_string();
    if let Some(unc) = rest.strip_prefix("UNC/") {
        return format!("file://{}", encoded(unc));
    }
    if let Some(unc) = rest.strip_prefix("//") {
        return format!("file://{}", encoded(unc));
    }
    let drive = rest.as_bytes();
    if drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':' {
        return format!("file:///{}", encoded(rest));
    }
    format!("file://{}", encoded(rest))
}

pub fn markdown_destination(from: &Path, to: &Path) -> String {
    let parent: Vec<_> = from
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .collect();
    let destination: Vec<_> = to.components().collect();
    let common = parent
        .iter()
        .zip(&destination)
        .take_while(|(a, b)| a == b)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..parent.len() {
        relative.push("..");
    }
    for component in &destination[common..] {
        relative.push(component.as_os_str());
    }
    utf8_percent_encode(&relative.to_string_lossy().replace('\\', "/"), RESERVED).to_string()
}

#[cfg(test)]
mod tests {
    use super::windows_file_uri;

    #[test]
    fn windows_paths_become_file_uris_without_the_verbatim_prefix() {
        for (path, uri) in [
            (
                r"\\?\C:\Users\me\record #1.json",
                "file:///C:/Users/me/record%20%231.json",
            ),
            (r"C:\Users\me\a.json", "file:///C:/Users/me/a.json"),
            (
                r"\\?\D:\Notas (x)\é.pdf",
                "file:///D:/Notas%20%28x%29/%C3%A9.pdf",
            ),
            (r"\\?\UNC\srv\share\a b.pdf", "file://srv/share/a%20b.pdf"),
            (r"\\srv\share\a.pdf", "file://srv/share/a.pdf"),
        ] {
            assert_eq!(windows_file_uri(path), uri, "{path}");
            assert!(!uri.contains("%3F"));
        }
    }
}
