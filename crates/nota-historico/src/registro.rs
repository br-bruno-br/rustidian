//! Tipos públicos da versão e o formato binário compacto do registro guardado no banco.
//!
//! O registro é pequeno de propósito (uma nota com 1.000 versões tem 1.000 registros): números em
//! varint, ids como distância, sem rótulos. Formato (versão 1):
//!
//! ```text
//! byte 0     bit 0: 1 = diferença, 0 = quadro-chave (cópia cheia, em pedaços)
//!            bits 1-2: origem   bit 3: tem nome
//! varint     quando (ms desde 1970, UTC)
//! varint     tamanho do texto em bytes
//! 32 bytes   blake3 do texto inteiro
//! varint     distância até o quadro-chave mais antigo anterior (0 = não há)
//! [varint n + n bytes de nome]            se tem nome
//! quadro-chave: varint n + n chaves de 32 bytes (pedaços)
//! diferença:    varint distância até a versão base (a vizinha mais nova) + o resto = zstd com a base
//! ```

use crate::erro::{Erro, Result};
use crate::pedacos::Chave;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Identificador estável da nota, dado pelo chamador (não é o caminho: a nota pode ser renomeada).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotaId(pub String);

impl NotaId {
    pub fn novo(id: impl Into<String>) -> Self {
        NotaId(id.into())
    }
    pub fn como_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NotaId {
    fn from(s: &str) -> Self {
        NotaId(s.to_owned())
    }
}

/// Número da versão dentro da nota: cresce a cada versão gravada e nunca é reaproveitado
/// (as podadas somem, mas os números das outras não mudam).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersaoId(pub u64);

/// De onde veio a versão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origem {
    /// Digitada no editor (pausa, frase, salvar...).
    Editor,
    /// Mudou fora do aplicativo (outro programa).
    Externa,
    /// Veio do sync (outro aparelho).
    Sync,
    /// Gravada ao restaurar uma versão antiga.
    Restauracao,
}

impl Origem {
    fn codigo(self) -> u8 {
        match self {
            Origem::Editor => 0,
            Origem::Externa => 1,
            Origem::Sync => 2,
            Origem::Restauracao => 3,
        }
    }
    fn de_codigo(c: u8) -> Origem {
        match c & 3 {
            0 => Origem::Editor,
            1 => Origem::Externa,
            2 => Origem::Sync,
            _ => Origem::Restauracao,
        }
    }
}

/// O que se sabe de uma versão sem reconstruir o texto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersaoInfo {
    pub id: VersaoId,
    pub quando: SystemTime,
    pub origem: Origem,
    /// Tamanho do texto em bytes.
    pub tamanho: usize,
    /// Nome dado pelo usuário; versões com nome nunca são podadas.
    pub nome: Option<String>,
    /// blake3 do texto inteiro.
    pub impressao: [u8; 32],
}

impl VersaoInfo {
    /// A impressão em hexadecimal (64 caracteres).
    pub fn impressao_hex(&self) -> String {
        self.impressao.iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub(crate) fn ms_de(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

pub(crate) fn tempo_de(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}

/// Como a versão está guardada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Corpo {
    /// Cópia cheia, em pedaços endereçados pelo conteúdo.
    Cheia(Vec<Chave>),
    /// zstd do texto com a versão `base` (vizinha mais nova) como prefixo.
    Delta { base: u64, dados: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Registro {
    pub origem: Origem,
    pub quando_ms: u64,
    pub tamanho: u32,
    pub hash: [u8; 32],
    /// Id do quadro-chave mais antigo anterior a esta versão (0 = nenhum).
    pub chave_anterior: u64,
    pub nome: Option<String>,
    pub corpo: Corpo,
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

struct Leitor<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Leitor<'a> {
    fn trunc() -> Erro {
        Erro::Corrompido("registro de versão truncado".into())
    }
    fn byte(&mut self) -> Result<u8> {
        let v = *self.b.get(self.p).ok_or_else(Self::trunc)?;
        self.p += 1;
        Ok(v)
    }
    fn varint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            v |= u64::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(Erro::Corrompido("varint longo demais".into()))
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let fim = self.p.checked_add(n).ok_or_else(Self::trunc)?;
        let s = self.b.get(self.p..fim).ok_or_else(Self::trunc)?;
        self.p = fim;
        Ok(s)
    }
}

impl Registro {
    /// `id` é a versão deste registro (a chave no banco): ids de referência viram distância.
    pub fn codificar(&self, id: u64) -> Vec<u8> {
        let mut o = Vec::with_capacity(64 + 40);
        let delta = matches!(self.corpo, Corpo::Delta { .. });
        o.push(u8::from(delta) | (self.origem.codigo() << 1) | (u8::from(self.nome.is_some()) << 3));
        varint(&mut o, self.quando_ms);
        varint(&mut o, u64::from(self.tamanho));
        o.extend_from_slice(&self.hash);
        varint(
            &mut o,
            if self.chave_anterior == 0 {
                0
            } else {
                id - self.chave_anterior
            },
        );
        if let Some(n) = &self.nome {
            varint(&mut o, n.len() as u64);
            o.extend_from_slice(n.as_bytes());
        }
        match &self.corpo {
            Corpo::Cheia(chaves) => {
                varint(&mut o, chaves.len() as u64);
                for c in chaves {
                    o.extend_from_slice(c);
                }
            }
            Corpo::Delta { base, dados } => {
                varint(&mut o, base - id);
                o.extend_from_slice(dados);
            }
        }
        o
    }

    pub fn decodificar(id: u64, b: &[u8]) -> Result<Registro> {
        let mut l = Leitor { b, p: 0 };
        let b0 = l.byte()?;
        let delta = b0 & 1 == 1;
        let origem = Origem::de_codigo(b0 >> 1);
        let tem_nome = b0 & 8 != 0;
        let quando_ms = l.varint()?;
        let tamanho = u32::try_from(l.varint()?)
            .map_err(|_| Erro::Corrompido("tamanho de versão grande demais".into()))?;
        let hash: [u8; 32] = l.bytes(32)?.try_into().expect("32 bytes");
        let dist = l.varint()?;
        let chave_anterior = if dist == 0 {
            0
        } else {
            id.checked_sub(dist)
                .ok_or_else(|| Erro::Corrompido("quadro-chave anterior inválido".into()))?
        };
        let nome = if tem_nome {
            let n = l.varint()? as usize;
            Some(
                String::from_utf8(l.bytes(n)?.to_vec())
                    .map_err(|_| Erro::Corrompido("nome de versão não é UTF-8".into()))?,
            )
        } else {
            None
        };
        let corpo = if delta {
            let d = l.varint()?;
            let base = id
                .checked_add(d)
                .ok_or_else(|| Erro::Corrompido("base inválida".into()))?;
            Corpo::Delta {
                base,
                dados: b[l.p..].to_vec(),
            }
        } else {
            let n = l.varint()? as usize;
            if n > b.len() / 32 + 1 {
                return Err(Erro::Corrompido("lista de pedaços maior que o registro".into()));
            }
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(l.bytes(32)?.try_into().expect("32 bytes"));
            }
            Corpo::Cheia(v)
        };
        Ok(Registro {
            origem,
            quando_ms,
            tamanho,
            hash,
            chave_anterior,
            nome,
            corpo,
        })
    }

    pub fn info(&self, id: u64) -> VersaoInfo {
        VersaoInfo {
            id: VersaoId(id),
            quando: tempo_de(self.quando_ms),
            origem: self.origem,
            tamanho: self.tamanho as usize,
            nome: self.nome.clone(),
            impressao: self.hash,
        }
    }

    pub fn e_cheia(&self) -> bool {
        matches!(self.corpo, Corpo::Cheia(_))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn exemplo(corpo: Corpo, nome: Option<&str>) -> Registro {
        Registro {
            origem: Origem::Sync,
            quando_ms: 1_763_000_000_123,
            tamanho: 5034,
            hash: [7; 32],
            chave_anterior: 60,
            nome: nome.map(str::to_owned),
            corpo,
        }
    }

    #[test]
    fn ida_e_volta() {
        for r in [
            exemplo(Corpo::Cheia(vec![[1; 32], [2; 32]]), Some("rascunho 2")),
            exemplo(Corpo::Cheia(vec![]), None),
            exemplo(
                Corpo::Delta {
                    base: 125,
                    dados: vec![1, 2, 3],
                },
                None,
            ),
            exemplo(
                Corpo::Delta {
                    base: 101,
                    dados: vec![],
                },
                Some("é"),
            ),
        ] {
            let b = r.codificar(100);
            assert_eq!(Registro::decodificar(100, &b).unwrap(), r);
        }
    }

    #[test]
    fn truncado_da_erro_sem_pânico() {
        let b = exemplo(Corpo::Cheia(vec![[1; 32]]), Some("x")).codificar(100);
        for n in 0..b.len() {
            assert!(Registro::decodificar(100, &b[..n]).is_err(), "n={n}");
        }
    }

    #[test]
    fn registro_de_diferenca_e_pequeno() {
        let r = exemplo(
            Corpo::Delta {
                base: 101,
                dados: vec![0; 44],
            },
            None,
        );
        assert!(r.codificar(100).len() < 95);
    }
}
