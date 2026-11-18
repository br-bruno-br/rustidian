//! Ordem das listas: sem distinguir maiúscula de minúscula e com número "natural"
//! (`Nota 2` vem antes de `Nota 10`).

use std::cmp::Ordering;

/// Compara dois nomes para mostrar numa lista. Empate total cai na comparação exata, para a
/// ordem ser estável entre uma abertura e outra.
pub fn comparar(a: &str, b: &str) -> Ordering {
    natural(a, b).then_with(|| a.cmp(b))
}

fn natural(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();
    loop {
        match (ia.peek().copied(), ib.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = numero(&mut ia);
                let nb = numero(&mut ib);
                let sem_zeros = |s: &str| s.trim_start_matches('0').to_string();
                let (za, zb) = (sem_zeros(&na), sem_zeros(&nb));
                let o = za.len().cmp(&zb.len()).then_with(|| za.cmp(&zb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                ia.next();
                ib.next();
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

fn numero(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied() {
        if c.is_ascii_digit() {
            s.push(c);
            it.next();
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ordenar(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| comparar(a, b));
        v
    }

    #[test]
    fn numero_natural() {
        assert_eq!(
            ordenar(vec!["Nota 10", "Nota 2", "nota 1"]),
            vec!["nota 1", "Nota 2", "Nota 10"]
        );
    }

    #[test]
    fn ignora_caixa_e_desempata() {
        assert_eq!(ordenar(vec!["b", "A", "a"]), vec!["A", "a", "b"]);
    }

    #[test]
    fn zeros_a_esquerda() {
        assert_eq!(ordenar(vec!["x10", "x09", "x9"]), vec!["x09", "x9", "x10"]);
    }

    #[test]
    fn prefixo_vem_antes() {
        assert_eq!(
            ordenar(vec!["Sem título 2", "Sem título"]),
            vec!["Sem título", "Sem título 2"]
        );
    }
}
