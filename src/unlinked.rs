//! Unlinked mentions: other notes that name a note in plain text without linking to it.
//!
//! The idea and the whole-word rule come from Rustsidian's `mentions.rs`
//! (Copyright (c) 2026 Siddarth, github.com/siddarth1872004/RustSidian, MIT, see
//! THIRD_PARTY_NOTICES.txt). This version reuses our own link, math, code and front matter
//! parsing, so a match never sits inside anything the vault already treats as a link.

use std::ops::Range;

/// `text` lowercased char by char, keeping every byte offset: a char whose lowercase form has
/// another UTF-8 length (or is several chars) stays as it is.
pub fn lower_same_len(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let mut lower = c.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(l), None) if l.len_utf8() == c.len_utf8() => out.push(l),
            _ => out.push(c),
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Inline code ranges of one line, as byte ranges of that line.
pub(crate) fn inline_code(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor += 2;
            continue;
        }
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let width = bytes[cursor..].iter().take_while(|b| **b == b'`').count();
        let fence = "`".repeat(width);
        match line[cursor + width..].find(&fence) {
            Some(end) => {
                let end = cursor + width + end + width;
                out.push(cursor..end);
                cursor = end;
            }
            None => cursor += width,
        }
    }
    out
}

fn overlaps(range: &Range<usize>, others: &[Range<usize>]) -> bool {
    others
        .iter()
        .any(|other| other.start < range.end && other.end > range.start)
}

/// Whole-word, case-insensitive occurrences of any of `names` in `text`, as byte ranges.
///
/// Skipped: front matter, fenced and inline code, math, and every range in `excluded`
/// (the caller passes the note's wiki and Markdown links). Overlapping hits keep the longest.
pub fn spans(text: &str, names: &[String], excluded: &[Range<usize>]) -> Vec<Range<usize>> {
    let names: Vec<String> = names
        .iter()
        .map(|name| lower_same_len(name.trim()))
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    let body = crate::metadata::frontmatter(text)
        .ok()
        .flatten()
        .map_or(0, |properties| properties.range.end);
    let maths: Vec<Range<usize>> = crate::math::spans(text)
        .into_iter()
        .map(|math| math.range)
        .collect();
    let mut hits: Vec<Range<usize>> = Vec::new();
    for line in crate::vault::prose_lines(text) {
        let offset = line.as_ptr() as usize - text.as_ptr() as usize;
        if offset + line.len() <= body {
            continue;
        }
        let code = inline_code(line);
        let lower = lower_same_len(line);
        for name in &names {
            let mut from = 0;
            while let Some(found) = lower[from..].find(name.as_str()) {
                let (start, end) = (from + found, from + found + name.len());
                from = end;
                let before = line[..start].chars().next_back();
                let after = line[end..].chars().next();
                if before.is_some_and(is_word) || after.is_some_and(is_word) {
                    continue;
                }
                let absolute = offset + start..offset + end;
                if absolute.start < body
                    || overlaps(&(start..end), &code)
                    || overlaps(&absolute, &maths)
                    || overlaps(&absolute, excluded)
                {
                    continue;
                }
                hits.push(absolute);
            }
        }
    }
    hits.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut out: Vec<Range<usize>> = Vec::new();
    for hit in hits {
        if out.last().is_some_and(|last| hit.start < last.end) {
            continue;
        }
        out.push(hit);
    }
    out
}

/// The wikilink that replaces a mention: `[[target]]` when the shown text is the target
/// itself, `[[target|shown]]` otherwise, so the note keeps reading the same.
pub fn link_text(target: &str, shown: &str) -> String {
    if target == shown {
        format!("[[{target}]]")
    } else {
        format!("[[{target}|{shown}]]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn found<'a>(text: &'a str, list: &[&str]) -> Vec<&'a str> {
        spans(text, &names(list), &[])
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn whole_words_only_and_case_insensitive() {
        assert_eq!(
            found("Bob met bob and Bobby at BOB's", &["Bob"]),
            ["Bob", "bob", "BOB"]
        );
    }

    #[test]
    fn keeps_offsets_with_accents() {
        let text = "Falei com a João Silva ontem; JOÃO SILVA respondeu.";
        assert_eq!(found(text, &["João Silva"]), ["João Silva", "JOÃO SILVA"]);
    }

    #[test]
    fn skips_front_matter_code_and_math() {
        let text =
            "---\ntitle: Projeto\n---\nO Projeto anda.\n`Projeto`\n```\nProjeto\n```\n$Projeto$\n";
        let ranges = spans(text, &names(&["Projeto"]), &[]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "Projeto");
        assert!(text[..ranges[0].start].ends_with("O "));
    }

    #[test]
    fn skips_excluded_link_ranges() {
        let text = "Veja [[Projeto]] e o Projeto.";
        let link = 5..16;
        assert_eq!(&text[link.clone()], "[[Projeto]]");
        let ranges = spans(text, &names(&["Projeto"]), &[link]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, text.rfind("Projeto").unwrap());
    }

    #[test]
    fn longest_name_wins_on_overlap() {
        assert_eq!(
            found("Loja Replay abriu", &["Loja", "Loja Replay"]),
            ["Loja Replay"]
        );
    }

    #[test]
    fn empty_names_find_nothing() {
        assert!(found("qualquer texto", &["", "  "]).is_empty());
    }

    #[test]
    fn link_keeps_the_shown_text() {
        assert_eq!(link_text("Projeto", "Projeto"), "[[Projeto]]");
        assert_eq!(link_text("Projeto", "projeto"), "[[Projeto|projeto]]");
    }
}
