//! `RemotoMemoria`: um servidor inteiro em memória, para testes, com injeção de falhas.
//!
//! Vários aparelhos compartilham o mesmo servidor (`outro_aparelho`) e cada um tem o seu
//! próprio plano de falhas. Uma falha **antes** da chamada não chega ao servidor; uma falha
//! **depois** muda o servidor mas perde a resposta (o caso difícil: o cliente não sabe se deu certo).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::protocolo::{
    BLOB_MAXIMO, CHAVE_MAXIMA, Conflito409, ItemRemoto, Mudancas, PAGINA_MUDANCAS, hex64,
    sha256_hex,
};
use crate::remoto::{EnvioChave, ErroRemoto, Remoto};

/// Qual chamada da trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chamada {
    Mudancas,
    BaixarBlob,
    EnviarBlob,
    Publicar,
}

/// Quando a falha acontece em relação ao efeito no servidor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quando {
    Antes,
    Depois,
}

/// "A chamada de número `n` (0, 1, 2, ... deste aparelho) cai `quando`."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Falha {
    pub n: usize,
    pub quando: Quando,
}

#[derive(Default)]
struct Central {
    itens: BTreeMap<String, ItemRemoto>,
    blobs: HashMap<String, Vec<u8>>,
    seq: u64,
    chave: Option<Vec<u8>>,
}

type Gancho = Box<dyn FnMut(Chamada) + Send>;

/// Servidor em memória + o plano de falhas deste aparelho.
pub struct RemotoMemoria {
    central: Arc<Mutex<Central>>,
    falhas: Vec<Falha>,
    chamadas: usize,
    pagina: usize,
    gancho: Option<Gancho>,
}

impl Default for RemotoMemoria {
    fn default() -> Self {
        Self::novo()
    }
}

impl RemotoMemoria {
    pub fn novo() -> Self {
        Self {
            central: Arc::new(Mutex::new(Central::default())),
            falhas: Vec::new(),
            chamadas: 0,
            pagina: PAGINA_MUDANCAS,
            gancho: None,
        }
    }

    /// Outro aparelho no mesmo servidor, com contador e plano de falhas próprios.
    pub fn outro_aparelho(&self) -> Self {
        Self {
            central: Arc::clone(&self.central),
            falhas: Vec::new(),
            chamadas: 0,
            pagina: self.pagina,
            gancho: None,
        }
    }

    /// Define as falhas deste aparelho e zera o contador de chamadas.
    pub fn injetar(&mut self, falhas: Vec<Falha>) {
        self.falhas = falhas;
        self.chamadas = 0;
    }

    /// Tamanho da página de `mudancas` (para testar a paginação).
    pub fn com_pagina(mut self, itens: usize) -> Self {
        self.pagina = itens.max(1);
        self
    }

    /// Função chamada antes de cada chamada, para o teste mexer no cofre no meio do ciclo.
    pub fn com_gancho(mut self, gancho: impl FnMut(Chamada) + Send + 'static) -> Self {
        self.gancho = Some(Box::new(gancho));
        self
    }

    /// Troca (ou tira) o gancho de um aparelho já criado.
    pub fn definir_gancho(&mut self, gancho: Option<Box<dyn FnMut(Chamada) + Send>>) {
        self.gancho = gancho;
    }

    /// Quantas chamadas este aparelho já fez desde o último `injetar`.
    pub fn chamadas(&self) -> usize {
        self.chamadas
    }

    /// O item do índice, para os testes conferirem o servidor.
    pub fn item(&self, id: &str) -> Option<ItemRemoto> {
        self.trava().itens.get(id).cloned()
    }

    /// Todos os blobs guardados (o que o servidor "enxerga").
    pub fn blobs(&self) -> Vec<Vec<u8>> {
        self.trava().blobs.values().cloned().collect()
    }

    fn trava(&self) -> MutexGuard<'_, Central> {
        self.central.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Conta a chamada, roda o gancho e devolve se há falha marcada para ela.
    fn entrar(&mut self, chamada: Chamada) -> (usize, Option<Quando>) {
        let n = self.chamadas;
        self.chamadas += 1;
        if let Some(g) = self.gancho.as_mut() {
            g(chamada);
        }
        let falha = self.falhas.iter().find(|f| f.n == n).map(|f| f.quando);
        (n, falha)
    }

    fn queda(chamada: Chamada, n: usize) -> ErroRemoto {
        ErroRemoto::Rede(format!("queda injetada na chamada {n} ({chamada:?})"))
    }
}

impl Remoto for RemotoMemoria {
    fn baixar_chave(&mut self) -> Result<Option<Vec<u8>>, ErroRemoto> {
        Ok(self.trava().chave.clone())
    }

    fn enviar_chave(&mut self, bytes: &[u8]) -> Result<EnvioChave, ErroRemoto> {
        if bytes.is_empty() || bytes.len() > CHAVE_MAXIMA {
            return Err(ErroRemoto::Protocolo(
                "cofre.chave com tamanho inválido".into(),
            ));
        }
        let mut c = self.trava();
        if c.chave.is_some() {
            return Ok(EnvioChave::JaExiste);
        }
        c.chave = Some(bytes.to_vec());
        Ok(EnvioChave::Gravada)
    }

    fn mudancas(&mut self, desde: u64) -> Result<Mudancas, ErroRemoto> {
        let (n, falha) = self.entrar(Chamada::Mudancas);
        if falha.is_some() {
            return Err(Self::queda(Chamada::Mudancas, n));
        }
        let c = self.trava();
        let mut itens: Vec<ItemRemoto> = c
            .itens
            .values()
            .filter(|i| i.seq > desde)
            .cloned()
            .collect();
        itens.sort_by_key(|i| i.seq);
        let mais = itens.len() > self.pagina;
        itens.truncate(self.pagina);
        let seq = itens.last().map_or(desde, |i| i.seq);
        Ok(Mudancas { itens, seq, mais })
    }

    fn baixar_blob(&mut self, hash: &str) -> Result<Vec<u8>, ErroRemoto> {
        let (n, falha) = self.entrar(Chamada::BaixarBlob);
        if falha.is_some() {
            return Err(Self::queda(Chamada::BaixarBlob, n));
        }
        if !hex64(hash) {
            return Err(ErroRemoto::Protocolo("hash inválido".into()));
        }
        self.trava()
            .blobs
            .get(hash)
            .cloned()
            .ok_or(ErroRemoto::NaoEncontrado)
    }

    fn enviar_blob(&mut self, bytes: &[u8]) -> Result<String, ErroRemoto> {
        let (n, falha) = self.entrar(Chamada::EnviarBlob);
        if falha == Some(Quando::Antes) {
            return Err(Self::queda(Chamada::EnviarBlob, n));
        }
        if bytes.len() > BLOB_MAXIMO {
            return Err(ErroRemoto::Protocolo("blob grande demais".into()));
        }
        let hash = sha256_hex(bytes);
        self.trava()
            .blobs
            .entry(hash.clone())
            .or_insert_with(|| bytes.to_vec());
        if falha == Some(Quando::Depois) {
            return Err(Self::queda(Chamada::EnviarBlob, n));
        }
        Ok(hash)
    }

    fn publicar(
        &mut self,
        id: &str,
        versao_base: u64,
        hash: &str,
        apagado: bool,
    ) -> Result<Result<u64, Conflito409>, ErroRemoto> {
        let (n, falha) = self.entrar(Chamada::Publicar);
        if falha == Some(Quando::Antes) {
            return Err(Self::queda(Chamada::Publicar, n));
        }
        if !hex64(id) || (!apagado && !hex64(hash)) {
            return Err(ErroRemoto::Protocolo("id ou hash inválido".into()));
        }
        let resultado = {
            let mut c = self.trava();
            if !apagado && !c.blobs.contains_key(hash) {
                return Err(ErroRemoto::Protocolo("blob desconhecido".into()));
            }
            let atual = c.itens.get(id).cloned();
            let versao_atual = atual.as_ref().map_or(0, |i| i.versao);
            if versao_atual != versao_base {
                Err(Conflito409 {
                    versao_atual,
                    hash_blob: atual.as_ref().and_then(|i| i.hash_blob.clone()),
                    apagado: atual.is_some_and(|i| i.apagado),
                })
            } else {
                c.seq += 1;
                let seq = c.seq;
                let versao = versao_atual + 1;
                c.itens.insert(
                    id.to_string(),
                    ItemRemoto {
                        id: id.to_string(),
                        versao,
                        hash_blob: (!apagado).then(|| hash.to_string()),
                        apagado,
                        seq,
                    },
                );
                Ok(versao)
            }
        };
        if falha == Some(Quando::Depois) {
            return Err(Self::queda(Chamada::Publicar, n));
        }
        Ok(resultado)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn id(n: u8) -> String {
        sha256_hex(&[n])
    }

    #[test]
    fn publicar_conflita_com_versao_velha() {
        let mut r = RemotoMemoria::novo();
        let h = r.enviar_blob(b"x").unwrap();
        assert_eq!(r.publicar(&id(1), 0, &h, false).unwrap(), Ok(1));
        let c = r.publicar(&id(1), 0, &h, false).unwrap().unwrap_err();
        assert_eq!(c.versao_atual, 1);
        assert_eq!(c.hash_blob.as_deref(), Some(h.as_str()));
        assert_eq!(r.publicar(&id(1), 1, "", true).unwrap(), Ok(2));
        assert!(r.item(&id(1)).unwrap().apagado);
    }

    #[test]
    fn falha_depois_muda_o_servidor_e_perde_a_resposta() {
        let mut r = RemotoMemoria::novo();
        let h = r.enviar_blob(b"x").unwrap();
        r.injetar(vec![Falha {
            n: 0,
            quando: Quando::Depois,
        }]);
        assert!(r.publicar(&id(1), 0, &h, false).is_err());
        assert_eq!(r.item(&id(1)).unwrap().versao, 1);
    }

    #[test]
    fn falha_antes_nao_chega_ao_servidor() {
        let mut r = RemotoMemoria::novo();
        let h = r.enviar_blob(b"x").unwrap();
        r.injetar(vec![Falha {
            n: 0,
            quando: Quando::Antes,
        }]);
        assert!(r.publicar(&id(1), 0, &h, false).is_err());
        assert!(r.item(&id(1)).is_none());
    }

    #[test]
    fn chave_so_grava_uma_vez_e_o_outro_aparelho_enxerga() {
        let mut a = RemotoMemoria::novo();
        let mut b = a.outro_aparelho();
        assert_eq!(a.baixar_chave().unwrap(), None);
        assert_eq!(a.enviar_chave(b"um").unwrap(), EnvioChave::Gravada);
        assert_eq!(b.enviar_chave(b"dois").unwrap(), EnvioChave::JaExiste);
        assert_eq!(b.baixar_chave().unwrap().as_deref(), Some(&b"um"[..]));
        assert!(a.enviar_chave(b"").is_err());
    }

    #[test]
    fn paginacao_entrega_tudo_em_ordem() {
        let mut r = RemotoMemoria::novo().com_pagina(2);
        let h = r.enviar_blob(b"x").unwrap();
        for n in 0..5 {
            r.publicar(&id(n), 0, &h, false).unwrap().unwrap();
        }
        let mut cursor = 0;
        let mut vistos = 0;
        loop {
            let m = r.mudancas(cursor).unwrap();
            vistos += m.itens.len();
            cursor = m.seq;
            if !m.mais {
                break;
            }
        }
        assert_eq!(vistos, 5);
        assert!(r.mudancas(cursor).unwrap().itens.is_empty());
    }
}
