//! Settings → Appearance → Font, as in Obsidian: an installed family for the
//! interface, for note text and for code. An empty name keeps Rustidian's own font.
use eframe::egui::{self, FontFamily};
use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    pub interface: String,
    pub text: String,
    pub monospace: String,
}

impl Fonts {
    /// Obsidian's `appearance.json` holds CSS font lists; the first family is the choice.
    pub fn from_obsidian(appearance: &serde_json::Value) -> Self {
        let first = |key: &str| {
            appearance
                .get(key)
                .and_then(serde_json::Value::as_str)
                .and_then(|list| list.split(',').next())
                .map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
                .unwrap_or_default()
        };
        Self {
            interface: first("interfaceFontFamily"),
            text: first("textFontFamily"),
            monospace: first("monospaceFontFamily"),
        }
    }
}

/// The faces the interface uses; note text has its own copy of each, `text-<face>`.
const FACES: [&str; 4] = ["regular", "semibold", "bold", "italic"];

/// The note-text family for an interface face.
pub fn text(face: &str) -> FontFamily {
    FontFamily::Name(format!("text-{face}").into())
}

fn interface(face: &str) -> FontFamily {
    match face {
        "regular" => FontFamily::Proportional,
        face => FontFamily::Name(face.into()),
    }
}

/// The families Windows ships, with the files of each face in `%WINDIR%\Fonts`:
/// (family, monospace, regular, semibold, bold, italic). Windows has no `fc-list`, and the
/// family names inside the font files are not read here, so the well-known ones are listed.
#[cfg(any(windows, test))]
const WINDOWS_FAMILIES: &[(&str, bool, &str, &str, &str, &str)] = &[
    (
        "Segoe UI",
        false,
        "segoeui.ttf",
        "seguisb.ttf",
        "segoeuib.ttf",
        "segoeuii.ttf",
    ),
    ("Segoe UI Emoji", false, "seguiemj.ttf", "", "", ""),
    (
        "Consolas",
        true,
        "consola.ttf",
        "",
        "consolab.ttf",
        "consolai.ttf",
    ),
    ("Arial", false, "arial.ttf", "", "arialbd.ttf", "ariali.ttf"),
    (
        "Calibri",
        false,
        "calibri.ttf",
        "",
        "calibrib.ttf",
        "calibrii.ttf",
    ),
    (
        "Cambria",
        false,
        "cambria.ttc",
        "",
        "cambriab.ttf",
        "cambriai.ttf",
    ),
    (
        "Georgia",
        false,
        "georgia.ttf",
        "",
        "georgiab.ttf",
        "georgiai.ttf",
    ),
    (
        "Verdana",
        false,
        "verdana.ttf",
        "",
        "verdanab.ttf",
        "verdanai.ttf",
    ),
    ("Tahoma", false, "tahoma.ttf", "", "tahomabd.ttf", ""),
    (
        "Times New Roman",
        false,
        "times.ttf",
        "",
        "timesbd.ttf",
        "timesi.ttf",
    ),
    (
        "Courier New",
        true,
        "cour.ttf",
        "",
        "courbd.ttf",
        "couri.ttf",
    ),
    ("Lucida Console", true, "lucon.ttf", "", "", ""),
];

/// The file of `family` in `face` inside `folder`, if Windows ships it and it is there.
#[cfg(any(windows, test))]
fn windows_file(folder: &Path, family: &str, face: &str) -> Option<PathBuf> {
    let entry = WINDOWS_FAMILIES
        .iter()
        .find(|entry| entry.0.eq_ignore_ascii_case(family.trim()))?;
    let name = match face {
        "semibold" => entry.3,
        "bold" => entry.4,
        "italic" => entry.5,
        _ => entry.2,
    };
    let file = folder.join(name);
    (!name.is_empty() && file.is_file()).then_some(file)
}

/// `%WINDIR%\Fonts`.
#[cfg(windows)]
fn windows_fonts() -> Option<PathBuf> {
    std::env::var_os("WINDIR")
        .or_else(|| std::env::var_os("SystemRoot"))
        .map(|windows| PathBuf::from(windows).join("Fonts"))
}

/// Installed families, by name, for the pickers; monospace ones only if `mono`.
#[cfg(windows)]
pub fn installed(mono: bool) -> &'static [String] {
    static ALL: OnceLock<Vec<String>> = OnceLock::new();
    static MONO: OnceLock<Vec<String>> = OnceLock::new();
    let list = |mono: bool| {
        let Some(folder) = windows_fonts() else {
            return Vec::new();
        };
        let mut names: Vec<String> = WINDOWS_FAMILIES
            .iter()
            .filter(|entry| {
                (!mono || entry.1) && windows_file(&folder, entry.0, "regular").is_some()
            })
            .map(|entry| entry.0.to_owned())
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names
    };
    if mono {
        MONO.get_or_init(|| list(true))
    } else {
        ALL.get_or_init(|| list(false))
    }
}

/// The installed file for `family` in `face`, or `None` when no such family is installed.
#[cfg(windows)]
fn file(family: &str, face: &str) -> Option<egui::FontData> {
    let path = windows_file(&windows_fonts()?, family, face)?;
    Some(egui::FontData::from_owned(std::fs::read(path).ok()?))
}

/// Installed families, by name, for the pickers; monospace ones only if `mono`.
#[cfg(not(windows))]
pub fn installed(mono: bool) -> &'static [String] {
    static ALL: OnceLock<Vec<String>> = OnceLock::new();
    static MONO: OnceLock<Vec<String>> = OnceLock::new();
    let list = |pattern: &str| {
        let output = std::process::Command::new("fc-list")
            .args([pattern, "family"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .unwrap_or_default();
        let mut names: Vec<String> = output
            .lines()
            .filter_map(|line| line.split(',').next())
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty() && !name.starts_with('.'))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    };
    if mono {
        MONO.get_or_init(|| list(":spacing=mono"))
    } else {
        ALL.get_or_init(|| list(":"))
    }
}

/// The installed file for `family` in `face`, or `None` when no such family is installed.
#[cfg(not(windows))]
fn file(family: &str, face: &str) -> Option<egui::FontData> {
    let style = match face {
        "semibold" => ":weight=semibold",
        "bold" => ":weight=bold",
        "italic" => ":slant=italic",
        _ => "",
    };
    let output = std::process::Command::new("fc-match")
        .args([
            "-f",
            "%{file}\n%{index}\n%{family}",
            &format!("{family}{style}"),
        ])
        .output()
        .ok()?;
    let output = String::from_utf8_lossy(&output.stdout);
    let mut lines = output.lines();
    let (path, index, families) = (lines.next()?, lines.next()?, lines.next()?);
    // fc-match always answers; a different family means this one is not installed.
    families
        .split(',')
        .any(|name| name.trim().eq_ignore_ascii_case(family.trim()))
        .then_some(())?;
    let mut data = egui::FontData::from_owned(std::fs::read(path).ok()?);
    data.index = index.parse().unwrap_or(0);
    Some(data)
}

/// Add the chosen families in front of Rustidian's own, and give note text
/// its own families. Returns the names that are not installed.
pub fn apply(definitions: &mut egui::FontDefinitions, fonts: &Fonts) -> Vec<String> {
    for face in FACES {
        let own = definitions
            .families
            .get(&interface(face))
            .cloned()
            .unwrap_or_default();
        definitions.families.insert(text(face), own);
    }
    let mut missing = Vec::new();
    for (name, family) in [
        (&fonts.interface, interface as fn(&str) -> FontFamily),
        (&fonts.text, text),
    ] {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let Some(regular) = file(name, "regular") else {
            missing.push(name.to_owned());
            continue;
        };
        for face in FACES {
            // A family without this face draws it with its regular file.
            let key = format!("{name}-{face}");
            let data = file(name, face).unwrap_or_else(|| regular.clone());
            definitions.font_data.insert(key.clone(), data.into());
            if let Some(list) = definitions.families.get_mut(&family(face)) {
                list.insert(0, key);
            }
        }
    }
    let name = fonts.monospace.trim();
    if !name.is_empty() {
        match file(name, "regular") {
            Some(data) => {
                definitions.font_data.insert(name.to_owned(), data.into());
                if let Some(list) = definitions.families.get_mut(&FontFamily::Monospace) {
                    list.insert(0, name.to_owned());
                }
            }
            None => missing.push(name.to_owned()),
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obsidian_font_lists_give_their_first_family() {
        let appearance = serde_json::json!({
            "textFontFamily": "\"Source Serif 4\", Georgia",
            "monospaceFontFamily": "JetBrains Mono",
        });
        assert_eq!(
            Fonts::from_obsidian(&appearance),
            Fonts {
                interface: String::new(),
                text: "Source Serif 4".into(),
                monospace: "JetBrains Mono".into(),
            }
        );
    }

    #[test]
    fn windows_families_resolve_to_files_in_the_fonts_folder() {
        let folder = tempfile::tempdir().unwrap();
        for name in ["segoeui.ttf", "segoeuib.ttf", "consola.ttf"] {
            std::fs::write(folder.path().join(name), b"font").unwrap();
        }
        let find = |family, face| windows_file(folder.path(), family, face);
        assert_eq!(
            find("segoe ui", "regular"),
            Some(folder.path().join("segoeui.ttf"))
        );
        assert_eq!(
            find("Segoe UI", "bold"),
            Some(folder.path().join("segoeuib.ttf"))
        );
        // A face the folder lacks, a family it lacks and an unknown family are all `None`.
        assert_eq!(find("Segoe UI", "italic"), None);
        assert_eq!(find("Segoe UI", "semibold"), None);
        assert_eq!(find("Arial", "regular"), None);
        assert_eq!(find("No Such Font 4096", "regular"), None);
        assert_eq!(
            find("Consolas", "regular"),
            Some(folder.path().join("consola.ttf"))
        );
    }

    #[test]
    fn note_text_keeps_rustidian_fonts_until_one_is_chosen() {
        let mut definitions = egui::FontDefinitions::default();
        definitions
            .families
            .insert(FontFamily::Name("bold".into()), vec!["Hack".into()]);
        let missing = apply(
            &mut definitions,
            &Fonts {
                text: "No Such Font 4096".into(),
                ..Default::default()
            },
        );
        assert_eq!(missing, ["No Such Font 4096"]);
        assert_eq!(definitions.families[&text("bold")], ["Hack"]);
        assert_eq!(
            definitions.families[&text("regular")],
            definitions.families[&FontFamily::Proportional]
        );
    }
}
