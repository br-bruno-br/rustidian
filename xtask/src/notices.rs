//! `language-notices`: acrescenta os avisos de licença de cada crate do build Linux que o
//! texto-base não cobre (antes `scripts/language-notices.py`).
//!
//! O texto-base é o `THIRD_PARTY_NOTICES.txt` verificado; um crate conta como coberto quando há
//! uma linha `<nome> <versão> — <licença>`. Os crates do build vêm de
//! `cargo tree --frozen --edges no-dev` para x86_64 Linux. Cada crate novo é lido do cache do
//! Cargo e conferido com o checksum do Cargo.lock. Offline depois de `cargo fetch --locked`.
//!
//! O dump de agradecimentos do two-face 0.5.1 é bincode 1 comprimido em zlib: dois vetores de
//! (enum u32 da licença, texto UTF-8, caminho UTF-8), com tamanhos u64.

use crate::ensure;
use crate::util::{Args, Result, fail, json_line, root, sha256_bytes, stdout_text};
use flate2::read::{GzDecoder, ZlibDecoder};
use regex::Regex;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

const TARGET: &str = "x86_64-unknown-linux-gnu";
/// Os avisos verificados da v0.1.0 que este comando estende; qualquer outro texto-base é recusado.
const BASELINE_NOTICES_SHA256: &str =
    "f63a6a19f1cca09c4b21dd8276953cd1a91ec979e1c67f3e98cab93ee1fde969";
const LICENSE_DIRS: [(&str, &str); 1] = [("epaint_default_fonts", "fonts/")];
const GRAMMARS: (&str, &str) = ("two-face", "0.5.1");
const SEPARATOR: &str = "======================================================================";

type Crate = (String, String);

fn covered_pattern() -> Regex {
    Regex::new(r"(?m)^([\w.-]+) ([0-9][^\s]*) — ").expect("regex fixa")
}

fn find_covered(text: &str) -> BTreeSet<Crate> {
    covered_pattern()
        .captures_iter(text)
        .map(|capture| (capture[1].to_owned(), capture[2].to_owned()))
        .collect()
}

/// Os crates compilados no build Linux, menos o próprio Rustidian.
fn build_crates(root: &Path) -> Result<BTreeSet<Crate>> {
    let tree = stdout_text(
        Command::new("cargo")
            .args([
                "tree", "--frozen", "--edges", "no-dev", "--target", TARGET, "--prefix", "none",
                "--format", "{p}",
            ])
            .current_dir(root),
    )?;
    let mut crates: BTreeSet<Crate> = tree
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((
                parts.next()?.to_owned(),
                parts.next()?.trim_start_matches('v').to_owned(),
            ))
        })
        .collect();
    let manifest: toml::Table = fs::read_to_string(root.join("Cargo.toml"))?.parse()?;
    let package = manifest["package"]
        .as_table()
        .ok_or("Cargo.toml sem [package]")?;
    crates.remove(&(
        package["name"].as_str().unwrap_or_default().to_owned(),
        package["version"].as_str().unwrap_or_default().to_owned(),
    ));
    Ok(crates)
}

fn crate_archive(name: &str, version: &str, checksum: &str) -> Result<Vec<u8>> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".cargo"))
        })
        .ok_or("Sem CARGO_HOME nem pasta pessoal")?;
    let cache = cargo_home.join("registry/cache");
    let mut registries: Vec<PathBuf> = fs::read_dir(&cache)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    registries.sort();
    for registry in registries {
        let path = registry.join(format!("{name}-{version}.crate"));
        if let Ok(data) = fs::read(&path)
            && sha256_bytes(&data) == checksum
        {
            return Ok(data);
        }
    }
    fail(format!(
        "{name} {version} with its locked checksum is not in the Cargo cache; run cargo fetch --locked"
    ))
}

struct Acknowledgements {
    data: Vec<u8>,
    offset: usize,
}

impl Acknowledgements {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let end = self.offset + count;
        ensure!(end <= self.data.len(), "Truncated acknowledgement data");
        let slice = &self.data[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }

    fn string(&mut self) -> Result<String> {
        let length = usize::try_from(self.u64()?)?;
        Ok(String::from_utf8(self.take(length)?.to_vec())?)
    }

    fn licenses(&mut self, expected: u64) -> Result<Vec<(String, String)>> {
        ensure!(
            self.u64()? == expected,
            "Pinned acknowledgement schema changed"
        );
        let mut licenses = Vec::new();
        for _ in 0..expected {
            let kind = self.u32()?;
            let text = self.string()?;
            let path = self.string()?;
            ensure!(
                kind <= 7 && !text.is_empty() && !path.is_empty(),
                "Invalid acknowledgement entry"
            );
            licenses.push((path, text));
        }
        Ok(licenses)
    }
}

fn grammar_licenses(dump: &[u8]) -> Result<Vec<(String, String)>> {
    let mut data = Vec::new();
    ZlibDecoder::new(dump).read_to_end(&mut data)?;
    let mut reader = Acknowledgements { data, offset: 0 };
    let syntaxes = reader.licenses(66)?;
    reader.licenses(12)?; // os temas não são usados: src/syntax.rs monta a paleta do Rustidian
    ensure!(
        reader.offset == reader.data.len(),
        "Unexpected bytes after pinned acknowledgement vectors"
    );
    Ok(syntaxes)
}

/// Os 128 caracteres de 0x80 a 0xFF do `mac_roman`.
const MAC_ROMAN_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéè\
êëíìîïñóòôöõúùûü\
†°¢£§•¶ß®©™´¨≠ÆØ\
∞±≤≥¥µ∂∑∏π∫ªºΩæø\
¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ\
–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ\
‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\
\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

fn decode_mac_roman(raw: &[u8]) -> String {
    let high: Vec<char> = MAC_ROMAN_HIGH.chars().collect();
    raw.iter()
        .map(|&byte| {
            if byte < 0x80 {
                char::from(byte)
            } else {
                high[usize::from(byte) - 0x80]
            }
        })
        .collect()
}

/// Copyright (nome 0) e licença (nome 13) da tabela `name` de uma fonte.
fn font_notices(data: &[u8]) -> Result<Vec<String>> {
    let be16 = |at: usize| -> Result<usize> {
        let bytes = data.get(at..at + 2).ok_or("Fonte truncada")?;
        Ok(usize::from(u16::from_be_bytes([bytes[0], bytes[1]])))
    };
    let be32 = |at: usize| -> Result<usize> {
        let bytes = data.get(at..at + 4).ok_or("Fonte truncada")?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    };
    let mut notices: Vec<String> = Vec::new();
    for index in 0..be16(4)? {
        let entry = 12 + index * 16;
        if data.get(entry..entry + 4) != Some(b"name") {
            continue;
        }
        let table = be32(entry + 8)?;
        let count = be16(table + 2)?;
        let storage = be16(table + 4)?;
        for record in 0..count {
            let at = table + 6 + record * 12;
            let (platform, name, length, offset) =
                (be16(at)?, be16(at + 6)?, be16(at + 8)?, be16(at + 10)?);
            if name == 0 || name == 13 {
                let start = table + storage + offset;
                let raw = data.get(start..start + length).ok_or("Fonte truncada")?;
                let text = if platform == 0 || platform == 3 {
                    let units: Vec<u16> = raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_be_bytes(*pair))
                        .collect();
                    String::from_utf16(&units)?
                } else {
                    decode_mac_roman(raw)
                };
                if !notices.contains(&text) {
                    notices.push(text);
                }
            }
        }
    }
    Ok(notices)
}

struct Section {
    text: String,
    grammars: usize,
    has_file: bool,
    fallback: Option<(String, PathBuf)>,
}

fn fallback_for(root: &Path, name: &str) -> Option<(String, PathBuf)> {
    let table = [
        ("ratex-", root.join("assets/fonts/LICENSE-RaTeX.txt")),
        ("gl_generator", root.join("vendor/LICENSE-APACHE")),
        ("khronos_api", root.join("vendor/LICENSE-APACHE")),
    ];
    table
        .into_iter()
        .find(|(start, _)| name.starts_with(start))
        .map(|(_, path)| {
            (
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
            )
        })
}

/// Uma seção com os arquivos de licença do crate, as notas das fontes e as gramáticas.
fn crate_notice(root: &Path, name: &str, version: &str, data: &[u8]) -> Result<Section> {
    let prefix = format!("{name}-{version}/");
    let mut archive = tar::Archive::new(GzDecoder::new(data));
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.to_string_lossy().replace('\\', "/");
        if let Some(relative) = path.strip_prefix(&prefix) {
            let mut body = Vec::new();
            entry.read_to_end(&mut body)?;
            files.insert(relative.to_owned(), body);
        }
    }
    let manifest: toml::Table = String::from_utf8(
        files
            .get("Cargo.toml")
            .ok_or("Crate sem Cargo.toml")?
            .clone(),
    )?
    .parse()?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or("Cargo.toml do crate sem [package]")?;
    let license_name = Regex::new(r"(?i)^(licen[cs]e|copying|copyright|notice|unlicense)")?;
    let mut names: Vec<String> = files
        .keys()
        .filter(|path| !path.contains('/') && license_name.is_match(path))
        .cloned()
        .collect();
    let dir = LICENSE_DIRS
        .iter()
        .find(|(crate_name, _)| *crate_name == name)
        .map_or("\0", |(_, dir)| dir);
    names.extend(
        files
            .keys()
            .filter(|path| path.starts_with(dir) && path.ends_with(".txt"))
            .cloned(),
    );
    if let Some(license_file) = package.get("license-file").and_then(toml::Value::as_str)
        && files.contains_key(license_file)
        && !names.iter().any(|existing| existing == license_file)
    {
        names.push(license_file.to_owned());
    }
    let mut texts = Vec::new();
    for path in &names {
        let body = String::from_utf8(files[path].clone())
            .map_err(|e| format!("{name} {version}: {path}: {e}"))?;
        texts.push((path.clone(), body.trim_end().to_owned()));
    }
    let mut fonts: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for (path, body) in &files {
        let lower = path.to_lowercase();
        if lower.ends_with(".ttf") || lower.ends_with(".otf") {
            let notices = font_notices(body)?;
            let file = Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            match fonts.iter_mut().find(|(known, _)| *known == notices) {
                Some((_, names)) => names.push(file),
                None => fonts.push((notices, vec![file])),
            }
        }
    }
    let grammars = if (name, version) == GRAMMARS {
        grammar_licenses(
            files
                .get("generated/acknowledgements_full.bin")
                .ok_or("two-face sem o dump de agradecimentos")?,
        )?
    } else {
        Vec::new()
    };
    let fallback = fallback_for(root, name);
    let license = package
        .get("license")
        .and_then(toml::Value::as_str)
        .unwrap_or("see license file");
    let mut section = format!("{SEPARATOR}\n{name} {version} — {license}\n");
    for (path, text) in &texts {
        section.push_str(&format!("\n--- {path} ---\n{text}\n"));
    }
    if texts.is_empty() {
        let see = fallback
            .as_ref()
            .map(|(file, _)| format!("; see {file} below"))
            .unwrap_or_default();
        section.push_str(&format!("\nThe package ships no license file{see}.\n"));
    }
    for (notices, paths) in &fonts {
        section.push_str(&format!(
            "\n> Font name-table notices: {}\n\n{}\n",
            paths.join(", "),
            notices.join("\n\n")
        ));
    }
    if !grammars.is_empty() {
        section.push_str(
            "\nMost of the code for generating theme and syntax dumps and curating them is taken from the bat project (https://github.com/sharkdp/bat).\n\
\nEmbedded grammar acknowledgements from two-face 0.5.1\nhttps://github.com/CosmicHorrorDev/two-face/blob/v0.5.1/generated/acknowledgements_full.md\n",
        );
        for (path, text) in &grammars {
            section.push_str(&format!(
                "\n{}\n{path}\n\n{}\n",
                "-".repeat(72),
                text.trim_end()
            ));
        }
    }
    Ok(Section {
        text: section,
        grammars: grammars.len(),
        has_file: !texts.is_empty(),
        fallback,
    })
}

pub fn run(args: &[String]) -> Result<()> {
    let args = Args::parse(args, &[])?;
    args.only(&["baseline-notices", "output"])?;
    let baseline_path = args.require_path("baseline-notices")?;
    let output_path = args.require_path("output")?;
    let root = root();
    let baseline = fs::read(&baseline_path)?;
    ensure!(
        sha256_bytes(&baseline) == BASELINE_NOTICES_SHA256,
        "{} is not the verified v0.1.0 notices file",
        baseline_path.display()
    );
    let mut text = String::from_utf8(baseline.clone())?;
    if let Some(rest) = text.strip_prefix("Rustidian alpha: ") {
        text = format!("Rustidian: {rest}");
    }
    // O texto-base lista os crates de fontes sem os textos de licença; eles voltam abaixo.
    let covered: BTreeSet<Crate> = find_covered(&text)
        .into_iter()
        .filter(|(name, _)| !LICENSE_DIRS.iter().any(|(known, _)| known == name))
        .collect();
    let lock: toml::Table = fs::read_to_string(root.join("Cargo.lock"))?.parse()?;
    let mut locked: BTreeMap<Crate, Option<String>> = BTreeMap::new();
    for package in lock["package"].as_array().ok_or("Cargo.lock sem pacotes")? {
        locked.insert(
            (
                package["name"].as_str().unwrap_or_default().to_owned(),
                package["version"].as_str().unwrap_or_default().to_owned(),
            ),
            package
                .get("checksum")
                .and_then(toml::Value::as_str)
                .map(str::to_owned),
        );
    }
    let crates = build_crates(&root)?;
    let to_add: Vec<&Crate> = crates.difference(&covered).collect();
    ensure!(
        !to_add
            .iter()
            .any(|(name, version)| name == GRAMMARS.0
                && (name.as_str(), version.as_str()) != GRAMMARS),
        "two-face changed version; update the pinned grammar acknowledgement reader"
    );
    let mut sections: Vec<String> = Vec::new();
    let mut fallbacks: Vec<(PathBuf, String, Vec<String>)> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut without_file: Vec<String> = Vec::new();
    let mut grammar_count = 0usize;
    for (name, version) in to_add {
        let Some(Some(checksum)) = locked.get(&(name.clone(), version.clone())) else {
            continue; // um crate de caminho que o texto-base não tem entra em "missing" abaixo
        };
        let section = crate_notice(
            &root,
            name,
            version,
            &crate_archive(name, version, checksum)?,
        )?;
        sections.push(section.text);
        grammar_count += section.grammars;
        added.push(format!("{name} {version}"));
        match (section.has_file, section.fallback) {
            (false, Some((file, path))) => {
                match fallbacks.iter_mut().find(|(known, _, _)| *known == path) {
                    Some((_, _, names)) => names.push(name.clone()),
                    None => fallbacks.push((path, file, vec![name.clone()])),
                }
            }
            (false, None) => without_file.push(format!("{name} {version}")),
            _ => {}
        }
    }
    for (path, file, names) in &fallbacks {
        sections.push(format!(
            "{SEPARATOR}\n{file}, the license text for {}\n\n{}\n",
            names.join(", "),
            fs::read_to_string(path)?.trim_end()
        ));
    }
    if !sections.is_empty() {
        text = format!(
            "{}\n\n\nCrates added since the baseline notices\n\n{}",
            text.trim_end_matches('\n'),
            sections.join("\n")
        );
    }
    let output = text.into_bytes();
    let covered_final = find_covered(&String::from_utf8_lossy(&output));
    let missing: Vec<String> = crates
        .difference(&covered_final)
        .map(|(name, version)| format!("{name} {version}"))
        .collect();
    if let Some(folder) = output_path.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(&output_path, &output)?;
    let not_in_build: Vec<String> = covered
        .difference(&crates)
        .map(|(name, version)| format!("{name} {version}"))
        .collect();
    println!(
        "{}",
        json_line(&json!({
            "baseline_sha256": sha256_bytes(&baseline),
            "build_crates": crates.len(),
            "added": added,
            "added_without_license_file": without_file,
            "grammar_licenses": grammar_count,
            "baseline_crates_not_in_build": not_in_build,
            "missing_notices": missing,
            "notices_sha256": sha256_bytes(&output),
        }))
    );
    if !missing.is_empty() {
        return fail(format!("Crates without a notice: {}", missing.join(", ")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabela_mac_roman_tem_128_caracteres() {
        assert_eq!(MAC_ROMAN_HIGH.chars().count(), 128);
        assert_eq!(decode_mac_roman(&[0x43, 0xA9, 0x8E]), "C©é");
    }

    #[test]
    fn cobertos_sao_lidos_das_linhas_de_cabecalho() {
        let text = "serde 1.0.1 — MIT OR Apache-2.0\nsolta\nrustidian-x 0.1 — MIT\n";
        let covered = find_covered(text);
        assert!(covered.contains(&("serde".to_owned(), "1.0.1".to_owned())));
        assert!(covered.contains(&("rustidian-x".to_owned(), "0.1".to_owned())));
        assert_eq!(covered.len(), 2);
    }

    fn bincode_text(out: &mut Vec<u8>, text: &str) {
        out.extend((text.len() as u64).to_le_bytes());
        out.extend(text.as_bytes());
    }

    #[test]
    fn leitor_bincode_confere_o_esquema_fixo() {
        let mut raw = Vec::new();
        raw.extend(2u64.to_le_bytes());
        for index in 0..2u32 {
            raw.extend(index.to_le_bytes());
            bincode_text(&mut raw, "texto");
            bincode_text(&mut raw, &format!("arquivo{index}"));
        }
        let mut reader = Acknowledgements {
            data: raw.clone(),
            offset: 0,
        };
        let licenses = reader.licenses(2).unwrap();
        assert_eq!(licenses[1], ("arquivo1".to_owned(), "texto".to_owned()));
        let mut wrong = Acknowledgements {
            data: raw,
            offset: 0,
        };
        assert!(wrong.licenses(3).is_err());
    }

    #[test]
    fn notas_da_tabela_name_da_fonte() {
        // Fonte mínima: cabeçalho sfnt com uma tabela `name` de um registro (Windows, nome 0).
        let text: Vec<u8> = "(c) Teste"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        let mut font = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        font.extend(b"name");
        font.extend([0, 0, 0, 0]);
        font.extend(28u32.to_be_bytes());
        font.extend(0u32.to_be_bytes());
        // tabela name em 28: formato, contagem, início do armazenamento
        font.extend([0, 0, 0, 1, 0, 18]);
        font.extend([0, 3, 0, 1, 4, 9, 0, 0]);
        font.extend((text.len() as u16).to_be_bytes());
        font.extend([0, 0]);
        font.extend(&text);
        assert_eq!(font_notices(&font).unwrap(), ["(c) Teste"]);
    }
}
