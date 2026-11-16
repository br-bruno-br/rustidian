//! Estado local do sincronizador, num banco redb (fora do cofre).
//!
//! - `base`: por id, o que os dois lados tinham de acordo (`EntradaBase`);
//! - `remoto`: por id, a última versão que o servidor disse que existe (a "caixa de entrada":
//!   o que ainda não entrou na base fica pendente até ser resolvido);
//! - `diario`: as intenções do plano em andamento, na ordem, gravadas **antes** de cada efeito;
//! - `meta`: o cursor `seq` das mudanças já recebidas;
//! - `log`: o que foi recusado e por quê (sem conteúdo de nota).
//!
//! Cada mudança de estado importante é uma transação: o fim de uma intenção e a atualização
//! da base entram juntos ou não entram.

use std::collections::BTreeMap;
use std::path::Path;

use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::erro::ErroSync;
use crate::motor::{EntradaBase, Hash};
use crate::protocolo::ItemRemoto;

const BASE: TableDefinition<&str, &[u8]> = TableDefinition::new("base");
const REMOTO: TableDefinition<&str, &[u8]> = TableDefinition::new("remoto");
const DIARIO: TableDefinition<u64, &[u8]> = TableDefinition::new("diario");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
const LOG: TableDefinition<u64, &str> = TableDefinition::new("log");

const META_SEQ: &str = "seq";
const META_DIARIO_PROX: &str = "diario_prox";
const META_LOG_PROX: &str = "log_prox";
/// Quantas linhas de log ficam guardadas.
const LOG_MAXIMO: u64 = 500;

/// Em que ponto está um envio. A fase `Enviado` só existe depois que o blob já está no servidor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaseEnvio {
    Inicio,
    Enviado { hash_blob: String, hash_texto: Hash },
}

/// A intenção de um efeito, gravada no diário antes de o efeito acontecer. Carrega tudo o que
/// é preciso para **repetir** o efeito depois de uma queda sem duplicar nem perder nada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intencao {
    Enviar {
        id: String,
        caminho: String,
        /// Versão do servidor sobre a qual esta publicação se apoia.
        vbase: u64,
        fase: FaseEnvio,
    },
    EnviarLapide {
        id: String,
        caminho: Option<String>,
        vbase: u64,
    },
    Baixar {
        id: String,
        caminho: String,
        rv: u64,
        hash_blob: String,
        hash_remoto: Hash,
        /// Hash que o arquivo tinha na varredura (`None`: não existia).
        esperado: Option<Hash>,
    },
    ApagarLocal {
        id: String,
        caminho: String,
        /// Versão remota que a lápide incorpora (`None`: não veio do servidor).
        rv: Option<u64>,
        esperado: Hash,
    },
    GravarConflito {
        caminho: String,
        nome: String,
        esperado: Hash,
    },
    BaixarComo {
        id: String,
        caminho_remoto: String,
        nome: String,
        rv: u64,
        hash_blob: String,
        hash_remoto: Hash,
    },
}

pub struct Estado {
    db: Database,
}

fn est(e: impl std::fmt::Display) -> ErroSync {
    ErroSync::Estado(e.to_string())
}

fn ler_json<T: for<'de> Deserialize<'de>>(b: &[u8]) -> Result<T, ErroSync> {
    serde_json::from_slice(b).map_err(est)
}

fn json<T: Serialize>(v: &T) -> Result<Vec<u8>, ErroSync> {
    serde_json::to_vec(v).map_err(est)
}

impl Estado {
    pub fn abrir(arquivo: &Path) -> Result<Self, ErroSync> {
        if let Some(pai) = arquivo.parent() {
            std::fs::create_dir_all(pai).map_err(est)?;
        }
        let db = Database::create(arquivo).map_err(est)?;
        let w = db.begin_write().map_err(est)?;
        {
            w.open_table(BASE).map_err(est)?;
            w.open_table(REMOTO).map_err(est)?;
            w.open_table(DIARIO).map_err(est)?;
            w.open_table(META).map_err(est)?;
            w.open_table(LOG).map_err(est)?;
        }
        w.commit().map_err(est)?;
        Ok(Self { db })
    }

    pub fn seq(&self) -> Result<u64, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(META).map_err(est)?;
        Ok(t.get(META_SEQ).map_err(est)?.map_or(0, |v| v.value()))
    }

    /// Guarda os itens que o servidor mandou (só se forem mais novos que o que já se sabe)
    /// e avança o cursor, tudo na mesma transação.
    pub fn guardar_remoto(&self, itens: &[ItemRemoto], seq: u64) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut t = w.open_table(REMOTO).map_err(est)?;
            for i in itens {
                let atual: Option<ItemRemoto> = match t.get(i.id.as_str()).map_err(est)? {
                    Some(v) => Some(ler_json(v.value())?),
                    None => None,
                };
                if atual.is_none_or(|a| i.versao > a.versao) {
                    t.insert(i.id.as_str(), json(i)?.as_slice()).map_err(est)?;
                }
            }
            let mut m = w.open_table(META).map_err(est)?;
            let atual = m.get(META_SEQ).map_err(est)?.map_or(0, |v| v.value());
            if seq > atual {
                m.insert(META_SEQ, seq).map_err(est)?;
            }
        }
        w.commit().map_err(est)
    }

    /// Atualiza só a "caixa de entrada" com o estado que um 409 revelou.
    pub fn guardar_item_remoto(&self, item: &ItemRemoto) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut t = w.open_table(REMOTO).map_err(est)?;
            let atual: Option<ItemRemoto> = match t.get(item.id.as_str()).map_err(est)? {
                Some(v) => Some(ler_json(v.value())?),
                None => None,
            };
            if atual.is_none_or(|a| item.versao > a.versao) {
                t.insert(item.id.as_str(), json(item)?.as_slice())
                    .map_err(est)?;
            }
        }
        w.commit().map_err(est)
    }

    pub fn base(&self) -> Result<BTreeMap<String, EntradaBase>, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(BASE).map_err(est)?;
        let mut m = BTreeMap::new();
        for linha in t.iter().map_err(est)? {
            let (k, v) = linha.map_err(est)?;
            m.insert(k.value().to_string(), ler_json(v.value())?);
        }
        Ok(m)
    }

    pub fn remotos(&self) -> Result<BTreeMap<String, ItemRemoto>, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(REMOTO).map_err(est)?;
        let mut m = BTreeMap::new();
        for linha in t.iter().map_err(est)? {
            let (k, v) = linha.map_err(est)?;
            m.insert(k.value().to_string(), ler_json(v.value())?);
        }
        Ok(m)
    }

    /// Grava (ou troca) uma entrada da base e, se vier, o estado remoto correspondente.
    pub fn gravar_base(
        &self,
        entrada: &EntradaBase,
        remoto: Option<&ItemRemoto>,
    ) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        Self::aplicar(&w, Some(entrada), remoto)?;
        w.commit().map_err(est)
    }

    fn aplicar(
        w: &redb::WriteTransaction,
        entrada: Option<&EntradaBase>,
        remoto: Option<&ItemRemoto>,
    ) -> Result<(), ErroSync> {
        if let Some(e) = entrada {
            let mut t = w.open_table(BASE).map_err(est)?;
            t.insert(e.id.as_str(), json(e)?.as_slice()).map_err(est)?;
        }
        if let Some(i) = remoto {
            let mut t = w.open_table(REMOTO).map_err(est)?;
            let atual: Option<ItemRemoto> = match t.get(i.id.as_str()).map_err(est)? {
                Some(v) => Some(ler_json(v.value())?),
                None => None,
            };
            if atual.is_none_or(|a| i.versao >= a.versao) {
                t.insert(i.id.as_str(), json(i)?.as_slice()).map_err(est)?;
            }
        }
        Ok(())
    }

    /// Grava o plano inteiro no diário, em ordem, numa transação só.
    pub fn gravar_plano(&self, plano: &[Intencao]) -> Result<(), ErroSync> {
        if plano.is_empty() {
            return Ok(());
        }
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut m = w.open_table(META).map_err(est)?;
            let mut prox = m
                .get(META_DIARIO_PROX)
                .map_err(est)?
                .map_or(0, |v| v.value());
            let mut d = w.open_table(DIARIO).map_err(est)?;
            for i in plano {
                d.insert(prox, json(i)?.as_slice()).map_err(est)?;
                prox += 1;
            }
            m.insert(META_DIARIO_PROX, prox).map_err(est)?;
        }
        w.commit().map_err(est)
    }

    /// O diário pendente, na ordem em que foi gravado.
    pub fn diario(&self) -> Result<Vec<(u64, Intencao)>, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(DIARIO).map_err(est)?;
        let mut v = Vec::new();
        for linha in t.iter().map_err(est)? {
            let (k, val) = linha.map_err(est)?;
            v.push((k.value(), ler_json(val.value())?));
        }
        Ok(v)
    }

    pub fn diario_vazio(&self) -> Result<bool, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(DIARIO).map_err(est)?;
        t.is_empty().map_err(est)
    }

    /// Troca a intenção (muda de fase) sem terminá-la.
    pub fn atualizar_intencao(&self, chave: u64, i: &Intencao) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut d = w.open_table(DIARIO).map_err(est)?;
            d.insert(chave, json(i)?.as_slice()).map_err(est)?;
        }
        w.commit().map_err(est)
    }

    /// Termina a intenção: tira do diário e, na mesma transação, atualiza base e remoto.
    pub fn concluir(
        &self,
        chave: u64,
        base: Option<&EntradaBase>,
        remoto: Option<&ItemRemoto>,
    ) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut d = w.open_table(DIARIO).map_err(est)?;
            d.remove(chave).map_err(est)?;
        }
        Self::aplicar(&w, base, remoto)?;
        w.commit().map_err(est)
    }

    /// Registra uma recusa (sem conteúdo de nota). Guarda só as últimas `LOG_MAXIMO`.
    pub fn registrar(&self, linha: &str) -> Result<(), ErroSync> {
        let w = self.db.begin_write().map_err(est)?;
        {
            let mut m = w.open_table(META).map_err(est)?;
            let prox = m.get(META_LOG_PROX).map_err(est)?.map_or(0, |v| v.value());
            m.insert(META_LOG_PROX, prox + 1).map_err(est)?;
            let mut l = w.open_table(LOG).map_err(est)?;
            l.insert(prox, linha).map_err(est)?;
            if prox >= LOG_MAXIMO {
                l.remove(prox - LOG_MAXIMO).map_err(est)?;
            }
        }
        w.commit().map_err(est)
    }

    pub fn log(&self) -> Result<Vec<String>, ErroSync> {
        let r = self.db.begin_read().map_err(est)?;
        let t = r.open_table(LOG).map_err(est)?;
        let mut v = Vec::new();
        for linha in t.iter().map_err(est)? {
            v.push(linha.map_err(est)?.1.value().to_string());
        }
        Ok(v)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn item(id: &str, versao: u64) -> ItemRemoto {
        ItemRemoto {
            id: id.into(),
            versao,
            hash_blob: Some("h".into()),
            apagado: false,
            seq: versao,
        }
    }

    #[test]
    fn remoto_e_cursor_so_avancam() {
        let d = tempfile::tempdir().unwrap();
        let e = Estado::abrir(&d.path().join("s.redb")).unwrap();
        e.guardar_remoto(&[item("a", 3)], 3).unwrap();
        e.guardar_remoto(&[item("a", 2)], 2).unwrap();
        assert_eq!(e.remotos().unwrap()["a"].versao, 3);
        assert_eq!(e.seq().unwrap(), 3);
    }

    #[test]
    fn diario_sobrevive_a_reabrir_e_conclui_com_a_base() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.redb");
        let plano = vec![
            Intencao::EnviarLapide {
                id: "a".into(),
                caminho: None,
                vbase: 1,
            },
            Intencao::EnviarLapide {
                id: "b".into(),
                caminho: None,
                vbase: 2,
            },
        ];
        {
            let e = Estado::abrir(&p).unwrap();
            e.gravar_plano(&plano).unwrap();
        }
        let e = Estado::abrir(&p).unwrap();
        let pend = e.diario().unwrap();
        assert_eq!(pend.len(), 2);
        assert_eq!(pend[0].1, plano[0]);
        let entrada = EntradaBase {
            id: "a".into(),
            caminho: None,
            versao: 2,
            hash: None,
        };
        e.concluir(pend[0].0, Some(&entrada), None).unwrap();
        assert_eq!(e.diario().unwrap().len(), 1);
        assert_eq!(e.base().unwrap()["a"], entrada);
        assert!(!e.diario_vazio().unwrap());
    }

    #[test]
    fn log_fica_limitado() {
        let d = tempfile::tempdir().unwrap();
        let e = Estado::abrir(&d.path().join("s.redb")).unwrap();
        for n in 0..(LOG_MAXIMO + 10) {
            e.registrar(&format!("linha {n}")).unwrap();
        }
        let l = e.log().unwrap();
        assert_eq!(l.len() as u64, LOG_MAXIMO);
        assert_eq!(l.last().unwrap(), &format!("linha {}", LOG_MAXIMO + 9));
    }
}
