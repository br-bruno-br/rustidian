//! Motor de reconciliação: **só decide**. Sem disco, sem rede, sem relógio, sem sorte.
//! A mesma entrada dá sempre a mesma saída; quem faz efeito é o `Sincronizador`.
//!
//! Para cada id de arquivo o motor compara três coisas:
//! - **local:** o que está no disco agora (hash BLAKE3 do texto claro);
//! - **base:** o que os dois lados tinham de acordo na última vez (hash e versão do servidor);
//! - **remoto:** uma versão do servidor mais nova que a base (já decifrada pelo executor).
//!
//! Tabela de casos (L = local, B = base, R = remoto novo):
//!
//! | L | R | ação |
//! |---|---|------|
//! | igual a B | nada | nada |
//! | mudou ou é novo | nada | `Enviar` |
//! | sumiu (B tinha conteúdo) | nada | `EnviarLapide` |
//! | igual a B | conteúdo | `Baixar` |
//! | mudou, **mesmo** conteúdo de R | conteúdo | `Baixar` (só alinha a base, sem conflito) |
//! | mudou, conteúdo diferente | conteúdo | `GravarConflito` (cópia local) e `Baixar` (R vira o principal) |
//! | sumiu | conteúdo | `Baixar` (a edição vence a exclusão: o arquivo volta) |
//! | igual a B | lápide | `ApagarLocal` (para a lixeira do cofre) |
//! | mudou ou é novo | lápide | `Enviar` (a edição vence a lápide, que é ignorada) |
//! | sumiu | lápide | nada (apagado nos dois lados; o executor só anota a versão) |
//!
//! Dois caminhos que só diferem na caixa não cabem no Windows nem no Android: o segundo
//! (em ordem de caminho) vira conflito com nome novo, nunca sobrescreve o primeiro.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::caminhos::{caminho_valido, dobrar, nome_conflito};

/// BLAKE3 do texto claro.
pub type Hash = [u8; 32];

/// Um arquivo que existe no cofre agora.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstadoLocal {
    /// Caminho normalizado (`/`, NFC).
    pub caminho: String,
    /// `ChaveCofre::id_de(caminho)`.
    pub id: String,
    pub hash: Hash,
}

/// O que a última sincronização deixou registrado para um id.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntradaBase {
    pub id: String,
    /// Caminho conhecido (pode faltar numa lápide de arquivo que este aparelho nunca viu).
    pub caminho: Option<String>,
    /// Versão do servidor que esta entrada incorpora.
    pub versao: u64,
    /// Hash do texto na versão sincronizada. `None`: lápide, ou conteúdo que este aparelho
    /// não guarda sob este caminho.
    pub hash: Option<Hash>,
}

/// A base de comparação mais o contexto de nomeação (para o motor continuar puro).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EstadoBase {
    /// Nome deste aparelho, usado nas cópias de conflito.
    pub aparelho: String,
    /// Carimbo `AAAA-MM-DD HHmm` do ciclo, usado nas cópias de conflito.
    pub carimbo: String,
    /// Por id.
    pub entradas: BTreeMap<String, EntradaBase>,
}

/// Uma versão do servidor que a base ainda não incorpora, **já decifrada** pelo executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MudancaRemota {
    pub id: String,
    pub versao: u64,
    pub apagado: bool,
    /// Caminho de dentro do blob decifrado. `None` na lápide.
    pub caminho: Option<String>,
    /// Hash do texto claro. `None` na lápide.
    pub hash: Option<Hash>,
}

/// O que fazer. O executor transforma cada uma em efeito.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acao {
    /// Publicar o arquivo local (novo ou editado).
    Enviar { caminho: String },
    /// Alinhar o arquivo local com a versão remota do id (escreve, ou só confere se já é igual).
    Baixar { id: String },
    /// Guardar a versão local de `caminho` sob `nome_conflito` (copia; não remove).
    GravarConflito {
        caminho: String,
        nome_conflito: String,
    },
    /// Mover o arquivo local para a lixeira do cofre.
    ApagarLocal { caminho: String },
    /// Publicar a lápide do id (o arquivo foi apagado aqui).
    EnviarLapide { id: String },
    /// Como `Baixar`, mas gravando sob `nome` porque o caminho remoto não cabe aqui (caixa).
    BaixarComo { id: String, nome: String },
}

fn mudou(l: &Hash, b: Option<&Hash>) -> bool {
    b != Some(l)
}

/// Decide as ações. Ver o quadro no topo do módulo.
pub fn reconciliar(
    local: &[EstadoLocal],
    base: &EstadoBase,
    remoto: &[MudancaRemota],
) -> Vec<Acao> {
    let mut acoes = Vec::new();

    // 1. Locais válidos, um por id, ordenados por (caixa dobrada, caminho).
    let mut validos: Vec<&EstadoLocal> = local
        .iter()
        .filter(|l| caminho_valido(&l.caminho))
        .collect();
    validos.sort_by(|a, b| (dobrar(&a.caminho), &a.caminho).cmp(&(dobrar(&b.caminho), &b.caminho)));
    validos.dedup_by(|a, b| a.id == b.id);

    // 2. Caminhos que só diferem na caixa: o primeiro fica, os outros viram conflito.
    let mut locais: BTreeMap<&str, &EstadoLocal> = BTreeMap::new();
    let mut ocupados: BTreeMap<String, &str> = BTreeMap::new();
    for l in validos {
        match ocupados.entry(dobrar(&l.caminho)) {
            Entry::Vacant(vaga) => {
                vaga.insert(&l.id);
                locais.insert(&l.id, l);
            }
            Entry::Occupied(_) => {
                acoes.push(Acao::GravarConflito {
                    caminho: l.caminho.clone(),
                    nome_conflito: nome_conflito(&l.caminho, &base.aparelho, &base.carimbo),
                });
                acoes.push(Acao::ApagarLocal {
                    caminho: l.caminho.clone(),
                });
            }
        }
    }

    // 3. Remotos que valem: mais novos que a base e com caminho aceitável. Um por id, o mais novo.
    let mut remotos: BTreeMap<&str, &MudancaRemota> = BTreeMap::new();
    for r in remoto {
        let versao_base = base.entradas.get(&r.id).map_or(0, |e| e.versao);
        if r.versao <= versao_base {
            continue;
        }
        if !r.apagado {
            let ok = r.hash.is_some() && r.caminho.as_deref().is_some_and(caminho_valido);
            if !ok {
                continue; // caminho malicioso ou blob estranho: recusado (o executor registra)
            }
        }
        match remotos.get(r.id.as_str()) {
            Some(atual) if atual.versao >= r.versao => {}
            _ => {
                remotos.insert(&r.id, r);
            }
        }
    }

    // 4. Por id, em ordem fixa.
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    ids.extend(locais.keys().copied());
    ids.extend(remotos.keys().copied());
    ids.extend(
        base.entradas
            .values()
            .filter(|e| e.hash.is_some())
            .map(|e| e.id.as_str()),
    );

    for id in ids {
        let l = locais.get(id).copied();
        let b = base.entradas.get(id);
        let b_hash = b.and_then(|e| e.hash.as_ref());
        let r = remotos.get(id).copied();

        match (l, r) {
            // Nada novo no servidor.
            (Some(l), None) => {
                if mudou(&l.hash, b_hash) {
                    acoes.push(Acao::Enviar {
                        caminho: l.caminho.clone(),
                    });
                }
            }
            (None, None) => {
                if b_hash.is_some() {
                    acoes.push(Acao::EnviarLapide { id: id.to_string() });
                }
            }
            // Lápide no servidor.
            (Some(l), Some(r)) if r.apagado => {
                if mudou(&l.hash, b_hash) {
                    acoes.push(Acao::Enviar {
                        caminho: l.caminho.clone(),
                    });
                } else {
                    acoes.push(Acao::ApagarLocal {
                        caminho: l.caminho.clone(),
                    });
                }
            }
            (None, Some(r)) if r.apagado => {}
            // Conteúdo novo no servidor.
            (Some(l), Some(r)) => {
                if r.caminho.as_deref() != Some(l.caminho.as_str()) {
                    continue; // mesmo id com caminhos diferentes: incoerente, não mexe
                }
                if mudou(&l.hash, b_hash) && Some(&l.hash) != r.hash.as_ref() {
                    acoes.push(Acao::GravarConflito {
                        caminho: l.caminho.clone(),
                        nome_conflito: nome_conflito(&l.caminho, &base.aparelho, &base.carimbo),
                    });
                }
                acoes.push(Acao::Baixar { id: id.to_string() });
            }
            (None, Some(r)) => {
                let caminho = r.caminho.as_deref().unwrap_or_default();
                let d = dobrar(caminho);
                match ocupados.get(&d) {
                    Some(dono) if *dono != id => acoes.push(Acao::BaixarComo {
                        id: id.to_string(),
                        nome: nome_conflito(caminho, &base.aparelho, &base.carimbo),
                    }),
                    _ => {
                        ocupados.insert(d, id);
                        acoes.push(Acao::Baixar { id: id.to_string() });
                    }
                }
            }
        }
    }
    acoes
}

#[cfg(test)]
mod testes {
    use super::*;

    fn h(n: u8) -> Hash {
        [n; 32]
    }
    fn local(c: &str, hash: u8) -> EstadoLocal {
        EstadoLocal {
            caminho: c.into(),
            id: format!("id:{c}"),
            hash: h(hash),
        }
    }
    fn id(c: &str) -> String {
        format!("id:{c}")
    }
    fn base(entradas: &[(&str, u64, Option<u8>)]) -> EstadoBase {
        EstadoBase {
            aparelho: "pc".into(),
            carimbo: "2026-11-15 1430".into(),
            entradas: entradas
                .iter()
                .map(|(c, v, hh)| {
                    (
                        id(c),
                        EntradaBase {
                            id: id(c),
                            caminho: Some((*c).into()),
                            versao: *v,
                            hash: hh.map(h),
                        },
                    )
                })
                .collect(),
        }
    }
    fn remota(c: &str, versao: u64, hash: u8) -> MudancaRemota {
        MudancaRemota {
            id: id(c),
            versao,
            apagado: false,
            caminho: Some(c.into()),
            hash: Some(h(hash)),
        }
    }
    fn lapide(c: &str, versao: u64) -> MudancaRemota {
        MudancaRemota {
            id: id(c),
            versao,
            apagado: true,
            caminho: None,
            hash: None,
        }
    }
    fn conflito(c: &str) -> Acao {
        Acao::GravarConflito {
            caminho: c.into(),
            nome_conflito: nome_conflito(c, "pc", "2026-11-15 1430"),
        }
    }

    #[test]
    fn nada_mudou() {
        assert!(reconciliar(&[local("a.md", 1)], &base(&[("a.md", 1, Some(1))]), &[]).is_empty());
        assert!(reconciliar(&[], &base(&[]), &[]).is_empty());
    }

    #[test]
    fn so_local_novo_e_editado() {
        let novo = reconciliar(&[local("a.md", 1)], &base(&[]), &[]);
        assert_eq!(
            novo,
            vec![Acao::Enviar {
                caminho: "a.md".into()
            }]
        );
        let editado = reconciliar(&[local("a.md", 2)], &base(&[("a.md", 1, Some(1))]), &[]);
        assert_eq!(
            editado,
            vec![Acao::Enviar {
                caminho: "a.md".into()
            }]
        );
        // recriado depois de uma lápide sincronizada
        let recriado = reconciliar(&[local("a.md", 3)], &base(&[("a.md", 2, None)]), &[]);
        assert_eq!(
            recriado,
            vec![Acao::Enviar {
                caminho: "a.md".into()
            }]
        );
    }

    #[test]
    fn so_local_apagado_vira_lapide() {
        let a = reconciliar(&[], &base(&[("a.md", 1, Some(1))]), &[]);
        assert_eq!(a, vec![Acao::EnviarLapide { id: id("a.md") }]);
        // lápide já sincronizada: nada a fazer
        assert!(reconciliar(&[], &base(&[("a.md", 2, None)]), &[]).is_empty());
    }

    #[test]
    fn so_remoto_novo_ou_editado() {
        let novo = reconciliar(&[], &base(&[]), &[remota("a.md", 1, 5)]);
        assert_eq!(novo, vec![Acao::Baixar { id: id("a.md") }]);
        let editado = reconciliar(
            &[local("a.md", 1)],
            &base(&[("a.md", 1, Some(1))]),
            &[remota("a.md", 2, 5)],
        );
        assert_eq!(editado, vec![Acao::Baixar { id: id("a.md") }]);
    }

    #[test]
    fn remoto_que_a_base_ja_tem_e_ignorado() {
        let a = reconciliar(
            &[local("a.md", 1)],
            &base(&[("a.md", 3, Some(1))]),
            &[remota("a.md", 3, 9), remota("a.md", 2, 8)],
        );
        assert!(a.is_empty());
    }

    #[test]
    fn so_remoto_apagado_apaga_local() {
        let a = reconciliar(
            &[local("a.md", 1)],
            &base(&[("a.md", 1, Some(1))]),
            &[lapide("a.md", 2)],
        );
        assert_eq!(
            a,
            vec![Acao::ApagarLocal {
                caminho: "a.md".into()
            }]
        );
    }

    #[test]
    fn mudou_nos_dois_lados_remoto_vira_principal_e_local_vira_conflito() {
        let a = reconciliar(
            &[local("a.md", 2)],
            &base(&[("a.md", 1, Some(1))]),
            &[remota("a.md", 2, 3)],
        );
        assert_eq!(a, vec![conflito("a.md"), Acao::Baixar { id: id("a.md") }]);
        let Acao::GravarConflito { nome_conflito, .. } = &a[0] else {
            panic!()
        };
        assert_eq!(nome_conflito, "a (conflito pc 2026-11-15 1430).md");
    }

    #[test]
    fn apagado_x_editado_a_edicao_vence_nos_dois_sentidos() {
        // local editou, remoto apagou: a lápide é ignorada e o arquivo é reenviado
        let a = reconciliar(
            &[local("a.md", 2)],
            &base(&[("a.md", 1, Some(1))]),
            &[lapide("a.md", 2)],
        );
        assert_eq!(
            a,
            vec![Acao::Enviar {
                caminho: "a.md".into()
            }]
        );
        // local apagou, remoto editou: o arquivo volta
        let b = reconciliar(&[], &base(&[("a.md", 1, Some(1))]), &[remota("a.md", 2, 3)]);
        assert_eq!(b, vec![Acao::Baixar { id: id("a.md") }]);
    }

    #[test]
    fn apagado_nos_dois_lados_nao_faz_nada() {
        let a = reconciliar(&[], &base(&[("a.md", 1, Some(1))]), &[lapide("a.md", 2)]);
        assert!(a.is_empty());
        // lápide de arquivo que nunca existiu aqui
        assert!(reconciliar(&[], &base(&[]), &[lapide("z.md", 4)]).is_empty());
    }

    #[test]
    fn local_novo_contra_lapide_remota_vence_o_local() {
        let a = reconciliar(&[local("a.md", 7)], &base(&[]), &[lapide("a.md", 3)]);
        assert_eq!(
            a,
            vec![Acao::Enviar {
                caminho: "a.md".into()
            }]
        );
    }

    #[test]
    fn criado_igual_nos_dois_lados_nao_e_conflito() {
        let a = reconciliar(&[local("a.md", 4)], &base(&[]), &[remota("a.md", 1, 4)]);
        assert_eq!(a, vec![Acao::Baixar { id: id("a.md") }]);
        // criado diferente nos dois lados: conflito
        let b = reconciliar(&[local("a.md", 4)], &base(&[]), &[remota("a.md", 1, 5)]);
        assert_eq!(b, vec![conflito("a.md"), Acao::Baixar { id: id("a.md") }]);
    }

    #[test]
    fn editado_igual_nos_dois_lados_nao_e_conflito() {
        let a = reconciliar(
            &[local("a.md", 9)],
            &base(&[("a.md", 1, Some(1))]),
            &[remota("a.md", 2, 9)],
        );
        assert_eq!(a, vec![Acao::Baixar { id: id("a.md") }]);
    }

    #[test]
    fn maiusculas_e_minusculas_locais_o_segundo_vira_conflito() {
        let a = reconciliar(&[local("a.md", 1), local("A.md", 2)], &base(&[]), &[]);
        // "A.md" < "a.md" em ordem de bytes: A.md fica, a.md sai do caminho
        assert_eq!(
            a,
            vec![
                conflito("a.md"),
                Acao::ApagarLocal {
                    caminho: "a.md".into()
                },
                Acao::Enviar {
                    caminho: "A.md".into()
                },
            ]
        );
    }

    #[test]
    fn maiusculas_e_minusculas_remoto_novo_vai_para_nome_de_conflito() {
        let a = reconciliar(&[local("a.md", 1)], &base(&[]), &[remota("A.md", 1, 2)]);
        assert_eq!(
            a,
            vec![
                Acao::BaixarComo {
                    id: id("A.md"),
                    nome: "A (conflito pc 2026-11-15 1430).md".into()
                },
                Acao::Enviar {
                    caminho: "a.md".into()
                },
            ]
        );
    }

    #[test]
    fn maiusculas_e_minusculas_entre_dois_remotos_novos() {
        let a = reconciliar(
            &[],
            &base(&[]),
            &[remota("a.md", 1, 1), remota("A.md", 1, 2)],
        );
        assert_eq!(
            a,
            vec![
                Acao::Baixar { id: id("A.md") },
                Acao::BaixarComo {
                    id: id("a.md"),
                    nome: "a (conflito pc 2026-11-15 1430).md".into()
                },
            ]
        );
    }

    #[test]
    fn caminho_malicioso_e_recusado() {
        for ruim in ["../x", "/etc/passwd", "C:/x", "a/../../b", ".trash/a.md"] {
            let r = MudancaRemota {
                id: "id:ruim".into(),
                versao: 1,
                apagado: false,
                caminho: Some(ruim.into()),
                hash: Some(h(1)),
            };
            assert!(reconciliar(&[], &base(&[]), &[r]).is_empty(), "{ruim}");
            let l = EstadoLocal {
                caminho: ruim.into(),
                id: "id:ruim".into(),
                hash: h(1),
            };
            assert!(reconciliar(&[l], &base(&[]), &[]).is_empty(), "{ruim}");
        }
        // blob sem caminho nem hash também não vira ação
        let sem = MudancaRemota {
            id: "id:z".into(),
            versao: 1,
            apagado: false,
            caminho: None,
            hash: None,
        };
        assert!(reconciliar(&[], &base(&[]), &[sem]).is_empty());
    }

    #[test]
    fn e_deterministico_e_nao_depende_da_ordem_da_entrada() {
        let ls = [local("b.md", 1), local("a.md", 2), local("c.md", 3)];
        let rs = [remota("d.md", 1, 4), remota("a.md", 1, 5)];
        let b = base(&[("c.md", 1, Some(3))]);
        let um = reconciliar(&ls, &b, &rs);
        let mut ls2 = ls.clone();
        ls2.reverse();
        let mut rs2 = rs.clone();
        rs2.reverse();
        assert_eq!(um, reconciliar(&ls2, &b, &rs2));
        assert_eq!(um, reconciliar(&ls, &b, &rs));
    }
}
