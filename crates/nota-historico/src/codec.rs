//! Compressão atrás de um trait, para trocar o zstd (C) por algo em Rust puro no Android se preciso.
//!
//! Duas operações: comprimir "solto" (pedaços dos quadros-chave) e comprimir **com uma base**
//! (a versão vizinha), que vira diferença e compressão de uma vez só.
//!
//! # Qual API do zstd para "comprimir com a versão vizinha como base"
//!
//! Medido com a nota sintética de 5 KB e edições de ~50 caracteres (ver `README.md`):
//!
//! | API                                              | nível 3: saída | tempo por chamada |
//! |--------------------------------------------------|----------------|-------------------|
//! | `CCtx::ref_prefix`, contexto novo a cada vez     | 44-49 B        | 70-520 us         |
//! | `CCtx::ref_prefix`, contexto reaproveitado       | 48 B           | 18-51 us          |
//! | `Compressor::with_dictionary`, novo a cada vez   | 44 B           | 87-975 us         |
//! | `Compressor::set_dictionary`, reaproveitado      | **44 B**       | **65-84 us**      |
//!
//! (Faixas porque o PC compilava outro projeto ao mesmo tempo; os pontos de dados completos
//! estão em `tests/medir_codec.rs`.)
//!
//! O custo grande é criar o contexto (tabelas de hash zeradas a cada chamada). O `ref_prefix`
//! pede que o prefixo viva tanto quanto o contexto (`'b: 'a`), o que impede guardar o contexto
//! dentro do codec sem `unsafe`. Por isso o codec guarda um `Compressor`/`Decompressor`
//! reaproveitado e carrega a base com `set_dictionary` (copia a base, sem empréstimo): sai o
//! menor tamanho e fica a ~70 us. A base é sempre texto UTF-8, que nunca começa com a assinatura
//! de dicionário formatado do zstd (`37 A4 30 EC`, inválida em UTF-8), então vale como conteúdo cru.

use crate::erro::{Erro, Result};
use std::sync::Mutex;
use zstd::bulk::{Compressor, Decompressor};

/// Compressão de dados pequenos, com ou sem uma base de referência.
///
/// Quem implementa precisa ser determinístico: os mesmos bytes e a mesma base dão a mesma saída
/// que `descomprimir*` desfaz. O nome é gravado no banco; abrir um banco de outro codec falha.
pub trait Codec: Send + Sync {
    /// Identificador estável gravado no banco (por exemplo `"zstd"`).
    fn nome(&self) -> &'static str;
    /// Comprime sem base.
    fn comprimir(&self, dados: &[u8]) -> Result<Vec<u8>>;
    /// Desfaz [`Codec::comprimir`]; `tamanho` é o tamanho original (para reservar memória e conferir).
    fn descomprimir(&self, comprimido: &[u8], tamanho: usize) -> Result<Vec<u8>>;
    /// Comprime `dados` sabendo de `base` (a versão vizinha): a saída só serve com a mesma base.
    fn comprimir_com_base(&self, dados: &[u8], base: &[u8]) -> Result<Vec<u8>>;
    /// Desfaz [`Codec::comprimir_com_base`].
    fn descomprimir_com_base(
        &self,
        comprimido: &[u8],
        base: &[u8],
        tamanho: usize,
    ) -> Result<Vec<u8>>;
}

/// Codec padrão: zstd (crate `zstd`, compila C). Reaproveita os contextos entre chamadas.
pub struct ZstdCodec {
    nivel: i32,
    plano: Mutex<Compressor<'static>>,
    com_base: Mutex<Compressor<'static>>,
    desc_plano: Mutex<Decompressor<'static>>,
    desc_com_base: Mutex<Decompressor<'static>>,
}

impl std::fmt::Debug for ZstdCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZstdCodec").field("nivel", &self.nivel).finish()
    }
}

fn erro_io(e: std::io::Error) -> Erro {
    Erro::Codec(e.to_string())
}

fn trava<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Se outra thread entrou em pânico segurando o contexto, ele continua utilizável: o zstd
    // reinicia a sessão a cada chamada.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ZstdCodec {
    /// Nível 3 é o padrão do zstd.
    pub const NIVEL_PADRAO: i32 = 3;

    pub fn novo(nivel: i32) -> Result<Self> {
        Ok(Self {
            nivel,
            plano: Mutex::new(Compressor::new(nivel).map_err(erro_io)?),
            com_base: Mutex::new(Compressor::new(nivel).map_err(erro_io)?),
            desc_plano: Mutex::new(Decompressor::new().map_err(erro_io)?),
            desc_com_base: Mutex::new(Decompressor::new().map_err(erro_io)?),
        })
    }
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self::novo(Self::NIVEL_PADRAO).expect("o zstd sempre cria contexto com nível válido")
    }
}

fn conferir(saida: Vec<u8>, tamanho: usize) -> Result<Vec<u8>> {
    if saida.len() != tamanho {
        return Err(Erro::Codec(format!(
            "tamanho {} diferente do gravado {tamanho}",
            saida.len()
        )));
    }
    Ok(saida)
}

impl Codec for ZstdCodec {
    fn nome(&self) -> &'static str {
        "zstd"
    }

    fn comprimir(&self, dados: &[u8]) -> Result<Vec<u8>> {
        trava(&self.plano).compress(dados).map_err(erro_io)
    }

    fn descomprimir(&self, comprimido: &[u8], tamanho: usize) -> Result<Vec<u8>> {
        let saida = trava(&self.desc_plano)
            .decompress(comprimido, tamanho)
            .map_err(erro_io)?;
        conferir(saida, tamanho)
    }

    fn comprimir_com_base(&self, dados: &[u8], base: &[u8]) -> Result<Vec<u8>> {
        let mut c = trava(&self.com_base);
        c.set_dictionary(self.nivel, base).map_err(erro_io)?;
        c.compress(dados).map_err(erro_io)
    }

    fn descomprimir_com_base(
        &self,
        comprimido: &[u8],
        base: &[u8],
        tamanho: usize,
    ) -> Result<Vec<u8>> {
        let mut d = trava(&self.desc_com_base);
        d.set_dictionary(base).map_err(erro_io)?;
        let saida = d.decompress(comprimido, tamanho).map_err(erro_io)?;
        conferir(saida, tamanho)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn texto(n: usize) -> Vec<u8> {
        (0..n)
            .map(|i| format!("frase número {} da nota, com acentuação. ", i * 7 % 13))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn solto_volta_igual() {
        let c = ZstdCodec::default();
        let t = texto(200);
        let z = c.comprimir(&t).unwrap();
        assert!(z.len() < t.len() / 4);
        assert_eq!(c.descomprimir(&z, t.len()).unwrap(), t);
    }

    #[test]
    fn com_base_vira_diferenca_pequena() {
        let c = ZstdCodec::default();
        let base = texto(200);
        let mut novo = base.clone();
        novo.extend_from_slice("e mais uma frase no fim.".as_bytes());
        let z = c.comprimir_com_base(&novo, &base).unwrap();
        assert!(z.len() < 150, "diferença com {} bytes", z.len());
        assert_eq!(c.descomprimir_com_base(&z, &base, novo.len()).unwrap(), novo);
    }

    #[test]
    fn vazio_e_tamanho_errado() {
        let c = ZstdCodec::default();
        let z = c.comprimir(b"").unwrap();
        assert_eq!(c.descomprimir(&z, 0).unwrap(), b"");
        let z = c.comprimir(b"abc abc abc").unwrap();
        assert!(c.descomprimir(&z, 5).is_err());
    }

    #[test]
    fn base_vazia_funciona() {
        let c = ZstdCodec::default();
        let z = c.comprimir_com_base(b"texto qualquer", b"").unwrap();
        assert_eq!(c.descomprimir_com_base(&z, b"", 14).unwrap(), b"texto qualquer");
    }
}
