//! Busca simples por nome e por conteúdo, sem índice: percorre o cofre a cada consulta.
//! Cada palavra digitada precisa aparecer (sem distinguir maiúscula); o resultado mostra
//! primeiro o que casou no nome e depois o que casou só no texto, com uma linha de amostra.

use super::cofre::{Cofre, Item, Tipo};
use super::ordem::comparar;

/// Quantos resultados no máximo (o resto some, e a tela avisa).
pub const MAX_ACHADOS: usize = 100;
/// Quantas pastas e notas olhar no máximo, para uma consulta nunca travar o celular.
const MAX_VISTOS: usize = 20_000;
/// Profundidade máxima de pastas.
const MAX_NIVEIS: usize = 24;
/// Tamanho da linha de amostra, em caracteres.
const TAM_TRECHO: usize = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Achado {
    pub item: Item,
    /// Linha da nota onde a palavra aparece (só quando casou pelo texto).
    pub trecho: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Resultado {
    pub achados: Vec<Achado>,
    /// `true` se parou no limite de resultados ou de arquivos olhados.
    pub truncado: bool,
}

/// Palavras da consulta, em minúsculas. Vazio = sem busca.
pub fn termos(consulta: &str) -> Vec<String> {
    consulta.split_whitespace().map(str::to_lowercase).collect()
}

fn tem_todos(texto: &str, termos: &[String]) -> bool {
    let t = texto.to_lowercase();
    termos.iter().all(|p| t.contains(p.as_str()))
}

/// A primeira linha do texto que contém a primeira palavra, cortada para caber na tela.
fn trecho(texto: &str, termos: &[String]) -> Option<String> {
    let primeira = termos.first()?;
    let linha = texto
        .lines()
        .find(|l| l.to_lowercase().contains(primeira.as_str()))?
        .trim();
    let mut s: String = linha.chars().take(TAM_TRECHO).collect();
    if linha.chars().count() > TAM_TRECHO {
        s.push('…');
    }
    Some(s)
}

/// Procura em todo o cofre.
pub fn buscar(cofre: &Cofre, consulta: &str) -> Resultado {
    let termos = termos(consulta);
    let mut r = Resultado::default();
    if termos.is_empty() {
        return r;
    }
    let mut por_nome: Vec<Achado> = Vec::new();
    let mut por_texto: Vec<Achado> = Vec::new();
    let mut vistos = 0usize;
    // Pilha de (pasta, nível): percorre o cofre inteiro sem recursão.
    let mut pilha = vec![(String::new(), 0usize)];
    'tudo: while let Some((pasta, nivel)) = pilha.pop() {
        let Ok(itens) = cofre.listar(&pasta) else {
            continue;
        };
        for item in itens {
            vistos += 1;
            if vistos > MAX_VISTOS {
                r.truncado = true;
                break 'tudo;
            }
            match item.tipo {
                Tipo::Pasta => {
                    if tem_todos(&item.nome, &termos) {
                        por_nome.push(Achado {
                            item: item.clone(),
                            trecho: None,
                        });
                    }
                    if nivel + 1 < MAX_NIVEIS {
                        pilha.push((item.caminho.clone(), nivel + 1));
                    }
                }
                Tipo::Nota => {
                    if tem_todos(&item.nome, &termos) {
                        por_nome.push(Achado { item, trecho: None });
                    } else if let Ok(texto) = cofre.ler_nota(&item.caminho)
                        && tem_todos(&texto, &termos)
                    {
                        let amostra = trecho(&texto, &termos);
                        por_texto.push(Achado {
                            item,
                            trecho: amostra,
                        });
                    }
                }
            }
        }
    }
    let ordem = |a: &Achado, b: &Achado| comparar(&a.item.caminho, &b.item.caminho);
    por_nome.sort_by(ordem);
    por_texto.sort_by(ordem);
    r.achados = por_nome;
    r.achados.extend(por_texto);
    if r.achados.len() > MAX_ACHADOS {
        r.achados.truncate(MAX_ACHADOS);
        r.truncado = true;
    }
    r
}

#[cfg(test)]
mod testes {
    use super::*;

    fn cofre_de_teste() -> (tempfile::TempDir, Cofre) {
        let t = tempfile::tempdir().unwrap();
        let c = Cofre::abrir(t.path().join("Cofre")).unwrap();
        c.salvar_nota("Compras.md", "# Lista\nleite\npão de açúcar\n")
            .unwrap();
        c.salvar_nota("Receita de pão.md", "farinha e água")
            .unwrap();
        c.criar_pasta("", "Trabalho").unwrap();
        c.salvar_nota("Trabalho/Ata.md", "Reunião com o PÃO Quente S.A.")
            .unwrap();
        c.criar_pasta("", "Pão doce").unwrap();
        (t, c)
    }

    fn caminhos(r: &Resultado) -> Vec<&str> {
        r.achados.iter().map(|a| a.item.caminho.as_str()).collect()
    }

    #[test]
    fn nome_vem_antes_do_texto_e_acha_pasta() {
        let (_t, c) = cofre_de_teste();
        let r = buscar(&c, "pão");
        assert_eq!(
            caminhos(&r),
            [
                "Pão doce",
                "Receita de pão.md",
                "Compras.md",
                "Trabalho/Ata.md"
            ]
        );
        // Só o que casou pelo texto tem amostra.
        assert_eq!(r.achados[0].trecho, None);
        assert_eq!(r.achados[1].trecho, None);
        assert_eq!(r.achados[2].trecho.as_deref(), Some("pão de açúcar"));
        assert_eq!(
            r.achados[3].trecho.as_deref(),
            Some("Reunião com o PÃO Quente S.A.")
        );
        assert!(!r.truncado);
    }

    #[test]
    fn todas_as_palavras_precisam_aparecer() {
        let (_t, c) = cofre_de_teste();
        assert_eq!(caminhos(&buscar(&c, "leite açúcar")), ["Compras.md"]);
        assert!(buscar(&c, "leite batata").achados.is_empty());
    }

    #[test]
    fn consulta_vazia_nao_busca() {
        let (_t, c) = cofre_de_teste();
        assert!(buscar(&c, "   ").achados.is_empty());
    }

    #[test]
    fn nao_olha_a_lixeira() {
        let (_t, c) = cofre_de_teste();
        c.para_a_lixeira_com("Compras.md", "2026-11-18 100000")
            .unwrap();
        assert!(buscar(&c, "leite").achados.is_empty());
    }

    #[test]
    fn corta_linha_comprida() {
        let longa = format!("alvo {}", "x".repeat(300));
        let t = trecho(&longa, &termos("alvo")).unwrap();
        assert_eq!(t.chars().count(), TAM_TRECHO + 1);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn limita_resultados() {
        let t = tempfile::tempdir().unwrap();
        let c = Cofre::abrir(t.path().join("Cofre")).unwrap();
        for i in 0..(MAX_ACHADOS + 5) {
            c.salvar_nota(&format!("n{i}.md"), "x").unwrap();
        }
        let r = buscar(&c, "n");
        assert_eq!(r.achados.len(), MAX_ACHADOS);
        assert!(r.truncado);
    }
}
