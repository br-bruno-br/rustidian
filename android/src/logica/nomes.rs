//! Nomes de notas e pastas: limpeza, extensão `.md` e nomes que não colidem.
//! Nenhuma função toca o disco: quem sabe o que já existe passa uma função `existe`.

use nota_sync::caminhos::caminho_valido;

pub const EXTENSAO: &str = ".md";

/// Limite de bytes do nome (o Android aceita 255 por componente; sobra espaço para a
/// extensão e para um sufixo ` 12`).
const MAX_BYTES: usize = 180;

/// `true` se o nome de arquivo é uma nota Markdown (`.md`, em qualquer caixa).
pub fn eh_nota(arquivo: &str) -> bool {
    arquivo.len() > EXTENSAO.len() && arquivo.to_lowercase().ends_with(EXTENSAO)
}

/// O título de uma nota é o nome do arquivo sem o `.md`. Um nome sem `.md` fica como está.
pub fn titulo_da_nota(arquivo: &str) -> &str {
    if eh_nota(arquivo) {
        &arquivo[..arquivo.len() - EXTENSAO.len()]
    } else {
        arquivo
    }
}

/// Nome de arquivo da nota a partir do título.
pub fn arquivo_da_nota(titulo: &str) -> String {
    format!("{titulo}{EXTENSAO}")
}

/// Última parte de um caminho com `/`.
pub fn ultimo_componente(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Pasta que contém `rel` (vazia = raiz do cofre).
pub fn pasta_de(rel: &str) -> &str {
    match rel.rfind('/') {
        Some(i) => &rel[..i],
        None => "",
    }
}

/// Junta pasta e nome (`""` = raiz).
pub fn juntar(pasta: &str, nome: &str) -> String {
    if pasta.is_empty() {
        nome.to_string()
    } else {
        format!("{pasta}/{nome}")
    }
}

/// Limpa o que o dono digitou como título de nota ou nome de pasta. `None` quando não sobra
/// nome válido. O resultado já está em NFC e é aceito por `caminho_valido` (com `.md`).
pub fn limpar_nome(entrada: &str) -> Option<String> {
    let mut s: String = entrada
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"|?*/".contains(c) || c == '\\' {
                '-'
            } else {
                c
            }
        })
        .collect();
    // O usuário pode ter digitado o `.md`: o título nunca leva a extensão.
    while eh_nota(&s) {
        s.truncate(s.len() - EXTENSAO.len());
    }
    // Nome escondido (começa com ponto) e ponto/espaço no fim não valem.
    let s = s
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim();
    if s.is_empty() {
        return None;
    }
    let mut s = nota_cripto::normalizar(s).ok()?;
    if s.len() > MAX_BYTES {
        let mut fim = MAX_BYTES;
        while !s.is_char_boundary(fim) {
            fim -= 1;
        }
        s.truncate(fim);
        s = s.trim_end().to_string();
    }
    if s.is_empty() || !caminho_valido(&arquivo_da_nota(&s)) {
        return None;
    }
    Some(s)
}

/// Primeiro nome livre: `base`, `base 2`, `base 3`... `existe` diz se o nome (sem extensão)
/// já está em uso.
pub fn nome_unico(existe: impl Fn(&str) -> bool, base: &str) -> String {
    if !existe(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidato = format!("{base} {n}");
        if !existe(&candidato) {
            return candidato;
        }
        n += 1;
    }
}

/// Título de nota nova: `Sem título`, `Sem título 2`...
pub const TITULO_NOVO: &str = "Sem título";
/// Nome de pasta nova.
pub const PASTA_NOVA: &str = "Nova pasta";

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn titulo_e_arquivo() {
        assert!(eh_nota("a.md"));
        assert!(eh_nota("A.MD"));
        assert!(!eh_nota(".md"));
        assert!(!eh_nota("a.txt"));
        assert_eq!(titulo_da_nota("Ideias.md"), "Ideias");
        assert_eq!(titulo_da_nota("sem-ext"), "sem-ext");
        assert_eq!(arquivo_da_nota("Ideias"), "Ideias.md");
    }

    #[test]
    fn caminhos() {
        assert_eq!(ultimo_componente("a/b/c.md"), "c.md");
        assert_eq!(ultimo_componente("c.md"), "c.md");
        assert_eq!(pasta_de("a/b/c.md"), "a/b");
        assert_eq!(pasta_de("c.md"), "");
        assert_eq!(juntar("", "x.md"), "x.md");
        assert_eq!(juntar("a", "x.md"), "a/x.md");
    }

    #[test]
    fn limpar_nomes() {
        assert_eq!(
            limpar_nome("  Reunião de hoje  ").as_deref(),
            Some("Reunião de hoje")
        );
        assert_eq!(limpar_nome("a/b").as_deref(), Some("a-b"));
        assert_eq!(limpar_nome("ideia.md").as_deref(), Some("ideia"));
        assert_eq!(limpar_nome("ideia.MD.md").as_deref(), Some("ideia"));
        assert_eq!(limpar_nome(".escondido").as_deref(), Some("escondido"));
        assert_eq!(limpar_nome("fim. ").as_deref(), Some("fim"));
        assert_eq!(limpar_nome("   "), None);
        assert_eq!(limpar_nome("..."), None);
        assert_eq!(limpar_nome("CON"), None);
    }

    #[test]
    fn limpar_normaliza_acento_decomposto() {
        // "é" como e + acento combinante vira o "é" inteiro (NFC).
        assert_eq!(limpar_nome("cafe\u{301}").as_deref(), Some("caf\u{e9}"));
    }

    #[test]
    fn limpar_corta_nome_enorme_sem_quebrar_caractere() {
        let longo = "ç".repeat(300);
        let r = limpar_nome(&longo).unwrap();
        assert!(r.len() <= MAX_BYTES);
        assert!(r.chars().all(|c| c == 'ç'));
    }

    #[test]
    fn unico() {
        let usados = ["Sem título", "Sem título 2"];
        let existe = |n: &str| usados.contains(&n);
        assert_eq!(nome_unico(existe, "Sem título"), "Sem título 3");
        assert_eq!(nome_unico(existe, "Outra"), "Outra");
    }
}
