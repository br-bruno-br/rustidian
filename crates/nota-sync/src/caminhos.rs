//! Regras de caminho do cofre, iguais em todo aparelho: o que é ignorado, o que é válido e
//! como se chama uma cópia de conflito.
//!
//! Os caminhos aqui são sempre relativos ao cofre, com `/` e em NFC (a saída de
//! `nota_cripto::normalizar`). Nenhuma função toca o disco.

/// Pastas na raiz do cofre que nunca sincronizam.
const PASTAS_IGNORADAS: [&str; 3] = [".trash", ".rustidian", ".git"];

/// Prefixo dos temporários do próprio sincronizador.
pub const PREFIXO_TEMP: &str = ".nota-sync-";

/// Nome (só o último componente) de arquivo temporário ou de sistema: nunca sincroniza.
pub fn eh_temporario(nome: &str) -> bool {
    let n = nome.to_lowercase();
    nome.starts_with(PREFIXO_TEMP)
        || n.ends_with(".tmp")
        || n.ends_with('~')
        || n.ends_with(".swp")
        || n.ends_with(".swx")
        || nome.starts_with(".#")
        || nome.starts_with("~$")
        || nome.starts_with(".~lock.")
        || n == ".ds_store"
        || n == "thumbs.db"
        || n == "desktop.ini"
}

/// `true` para o que o sincronizador não toca: `.trash/`, `.rustidian/`, `.git/`,
/// `.obsidian/workspace*` e temporários. Recebe o caminho já normalizado.
pub fn eh_ignorado(rel: &str) -> bool {
    let mut partes = rel.split('/');
    let primeira = partes.next().unwrap_or("");
    if PASTAS_IGNORADAS.contains(&primeira) {
        return true;
    }
    if primeira == ".obsidian"
        && let Some(segunda) = partes.next()
        && segunda.starts_with("workspace")
    {
        return true;
    }
    rel.rsplit('/').next().is_some_and(eh_temporario)
}

fn nome_reservado(componente: &str) -> bool {
    let base = componente
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit()
            && base.as_bytes()[3] != b'0')
}

fn componente_portavel(c: &str) -> bool {
    !c.is_empty()
        && !c.ends_with('.')
        && !c.ends_with(' ')
        && !nome_reservado(c)
        && !c
            .chars()
            .any(|ch| ch.is_control() || "<>:\"|?*\\".contains(ch))
}

/// Caminho que o cofre aceita sincronizar: já normalizado (`normalizar(c) == c`), portátil
/// entre Windows, Android e Linux, e fora das áreas ignoradas.
///
/// Um caminho que passa aqui nunca escapa do cofre: não tem `..`, nem raiz, nem unidade,
/// nem `:` (fluxos alternativos do NTFS), nem nomes reservados do Windows.
pub fn caminho_valido(rel: &str) -> bool {
    match nota_cripto::normalizar(rel) {
        Ok(n) if n == rel => {}
        _ => return false,
    }
    rel.split('/').all(componente_portavel) && !eh_ignorado(rel)
}

/// Forma "dobrada" para achar dois caminhos que só diferem na caixa (o Windows e o Android
/// não distinguem). Só serve para comparar.
pub fn dobrar(rel: &str) -> String {
    rel.to_lowercase()
}

/// Tira de um nome o que a máquina não deve levar para o nome de conflito.
fn limpar_aparelho(aparelho: &str) -> String {
    let s: String = aparelho
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() { "aparelho".into() } else { s }
}

/// `pasta/nome.md` vira `pasta/nome (conflito <aparelho> <carimbo>).md`. Um conflito de um
/// arquivo que já é cópia de conflito troca o sufixo antigo em vez de empilhar outro.
pub fn nome_conflito(caminho: &str, aparelho: &str, carimbo: &str) -> String {
    let (dir, arq) = match caminho.rfind('/') {
        Some(i) => (&caminho[..=i], &caminho[i + 1..]),
        None => ("", caminho),
    };
    let (mut stem, ext) = match arq.rfind('.') {
        Some(i) if i > 0 && arq.len() - i <= 17 && !arq[i..].contains(' ') => {
            (&arq[..i], &arq[i..])
        }
        _ => (arq, ""),
    };
    while stem.ends_with(')') {
        match stem.rfind(" (conflito ") {
            Some(i) if i > 0 => stem = &stem[..i],
            _ => break,
        }
    }
    format!(
        "{dir}{stem} (conflito {} {carimbo}){ext}",
        limpar_aparelho(aparelho)
    )
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ignorados() {
        for c in [
            ".trash/a.md",
            ".rustidian/x",
            ".git/config",
            ".obsidian/workspace.json",
            ".obsidian/workspaces.json",
            "pasta/a.md.tmp",
            "a.md~",
            ".nota-sync-1.tmp",
            "x/.#a.md",
            "~$a.docx",
            ".DS_Store",
        ] {
            assert!(eh_ignorado(c), "{c}");
        }
        for c in ["a.md", ".obsidian/app.json", "trash/a.md", "pasta/.trash/a"] {
            assert!(!eh_ignorado(c), "{c}");
        }
    }

    #[test]
    fn validos_e_invalidos() {
        for c in ["a.md", "pasta/sub/nota ç.md", ".obsidian/app.json"] {
            assert!(caminho_valido(c), "{c}");
        }
        for c in [
            "../x",
            "a/../b",
            "/etc/passwd",
            "C:/x",
            "a//b",
            "./a.md",
            "a\\b.md",
            "a:b.md",
            "CON.md",
            "dir/nul",
            "a.md.",
            "a ",
            "",
            ".trash/x",
            "a\0b",
            "x?.md",
        ] {
            assert!(!caminho_valido(c), "{c:?}");
        }
        // NFD não é a forma normalizada
        assert!(!caminho_valido("e\u{301}.md"));
    }

    #[test]
    fn nome_de_conflito() {
        assert_eq!(
            nome_conflito("pasta/nota.md", "pc", "2026-11-15 1430"),
            "pasta/nota (conflito pc 2026-11-15 1430).md"
        );
        assert_eq!(
            nome_conflito("sem-extensao", "pc", "T"),
            "sem-extensao (conflito pc T)"
        );
        assert_eq!(
            nome_conflito("a (conflito x T0).md", "pc", "T1"),
            "a (conflito pc T1).md"
        );
        assert_eq!(
            nome_conflito(".oculto", "a/b", "T"),
            ".oculto (conflito a_b T)"
        );
    }

    #[test]
    fn nome_de_conflito_e_valido() {
        let n = nome_conflito("x/nota.md", "Notebook do Bruno", "2026-11-15 1430");
        assert!(caminho_valido(&n), "{n}");
    }
}
