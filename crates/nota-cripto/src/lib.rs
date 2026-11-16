//! Miolo de criptografia do cofre. Ver docs/bruno/PLANO.md, seção "Criptografia".
//!
//! A crate só recebe e devolve bytes: não toca rede nem disco.
//!
//! Formatos (todos versionados no primeiro byte):
//! - `cofre.chave` v1: `versão(1) ‖ m_kib(u32 LE) ‖ t(u32 LE) ‖ p(u32 LE) ‖ sal(16) ‖ nonce(24) ‖
//!   XChaCha20-Poly1305(chave_frase, AAD = cabeçalho de 29 bytes, texto = K)`.
//! - blob v1: `versão(1) ‖ nonce(24) ‖ XChaCha20-Poly1305(k_conteudo, AAD = id ‖ versão,
//!   texto = len(u32 LE) ‖ caminho ‖ conteúdo)`.
#![forbid(unsafe_code)]

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::{ZeroizeOnDrop, Zeroizing};

/// Versão atual do formato do blob de arquivo.
const VERSAO_BLOB: u8 = 1;
/// Versão atual do formato do `cofre.chave`.
const VERSAO_COFRE: u8 = 1;
/// Maior conteúdo aceito por arquivo (50 MiB).
pub const TAMANHO_MAXIMO: usize = 50 * 1024 * 1024;
/// Maior caminho aceito, em bytes.
const CAMINHO_MAXIMO: usize = 4096;

const ROTULO_CONTEUDO: &[u8] = b"rustidian-bruno/conteudo/v1";
const ROTULO_NOME: &[u8] = b"rustidian-bruno/nome/v1";

const TAM_SAL: usize = 16;
const TAM_NONCE: usize = 24;
const TAM_TAG: usize = 16;
const TAM_CABECALHO_COFRE: usize = 1 + 12 + TAM_SAL;
const TAM_COFRE: usize = TAM_CABECALHO_COFRE + TAM_NONCE + 32 + TAM_TAG;
/// Menor blob possível: versão + nonce + tag + campo de tamanho do caminho.
const TAM_BLOB_MINIMO: usize = 1 + TAM_NONCE + TAM_TAG + 4;
/// Maior blob aceito ao decifrar (conteúdo e caminho máximos mais a sobrecarga).
const TAM_BLOB_MAXIMO: usize = TAMANHO_MAXIMO + CAMINHO_MAXIMO + TAM_BLOB_MINIMO;

/// Limites aceitos ao LER parâmetros do `cofre.chave` (evita alocação absurda vinda de arquivo adulterado).
const KDF_M_KIB_MAX: u32 = 1 << 20; // 1 GiB
const KDF_T_MAX: u32 = 64;
const KDF_P_MAX: u32 = 64;

/// Erros da crate. Nenhum guarda segredo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erro {
    /// A frase-senha não abre o `cofre.chave`.
    FraseErrada,
    /// Dado corrompido, truncado, trocado de lugar, chave errada ou parâmetros fora do aceitável.
    Adulterado,
    /// Primeiro byte de versão que esta crate não conhece.
    FormatoDesconhecido(u8),
    /// Caminho recusado por `normalizar`.
    CaminhoInvalido(String),
    /// Conteúdo acima de 50 MiB.
    TamanhoExcedido,
}

impl fmt::Display for Erro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Erro::FraseErrada => write!(f, "frase-senha errada"),
            Erro::Adulterado => write!(f, "dado adulterado, truncado ou chave errada"),
            Erro::FormatoDesconhecido(v) => write!(f, "versão de formato desconhecida: {v}"),
            Erro::CaminhoInvalido(m) => write!(f, "caminho inválido: {m}"),
            Erro::TamanhoExcedido => write!(f, "conteúdo acima do limite de 50 MiB"),
        }
    }
}

impl std::error::Error for Erro {}

/// Id de arquivo: HMAC-SHA256 em hex minúsculo (64 caracteres).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdArquivo(pub String);

/// Parâmetros do Argon2id gravados no `cofre.chave`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParamsKdf {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl ParamsKdf {
    /// Padrão do PLANO: 64 MiB, 3 passes, 1 faixa.
    pub(crate) const PADRAO: ParamsKdf = ParamsKdf {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// Só para testes.
    #[cfg(test)]
    pub(crate) const TESTE: ParamsKdf = ParamsKdf {
        m_kib: 8,
        t: 1,
        p: 1,
    };
}

/// Chave K do cofre (32 bytes). Zera a memória ao sair de escopo.
#[derive(ZeroizeOnDrop)]
pub struct ChaveCofre([u8; 32]);

impl fmt::Debug for ChaveCofre {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChaveCofre(..)")
    }
}

fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

/// Normaliza um caminho relativo: `\` vira `/`, NFC, tira `.` e barras repetidas.
/// Recusa vazio, absoluto (`/x`, `C:\x`, UNC), `..` e NUL.
pub fn normalizar(caminho: &str) -> Result<String, Erro> {
    let invalido = |m: &str| Err(Erro::CaminhoInvalido(m.to_string()));
    let c: String = caminho.replace('\\', "/").nfc().collect();
    if c.contains('\0') {
        return invalido("contém byte nulo");
    }
    if c.starts_with('/') {
        return invalido("caminho absoluto");
    }
    let mut partes: Vec<&str> = Vec::new();
    for p in c.split('/') {
        match p {
            "" | "." => {}
            ".." => return invalido("contém .."),
            _ => partes.push(p),
        }
    }
    if let Some(primeira) = partes.first() {
        let b = primeira.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return invalido("caminho absoluto (unidade)");
        }
    }
    if partes.is_empty() {
        return invalido("vazio");
    }
    let r = partes.join("/");
    if r.len() > CAMINHO_MAXIMO {
        return invalido("longo demais");
    }
    Ok(r)
}

fn aead(chave: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(chave.into())
}

fn nonce_aleatorio() -> [u8; TAM_NONCE] {
    let mut n = [0u8; TAM_NONCE];
    OsRng.fill_bytes(&mut n);
    n
}

fn sal_aleatorio() -> [u8; TAM_SAL] {
    let mut s = [0u8; TAM_SAL];
    OsRng.fill_bytes(&mut s);
    s
}

fn ler_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Chave de embrulho a partir da frase (NFC, para teclados diferentes concordarem).
fn derivar_frase(frase: &str, sal: &[u8], p: ParamsKdf) -> Result<Zeroizing<[u8; 32]>, Erro> {
    if p.m_kib > KDF_M_KIB_MAX || p.t > KDF_T_MAX || p.p > KDF_P_MAX {
        return Err(Erro::Adulterado);
    }
    let params = Params::new(p.m_kib, p.t, p.p, Some(32)).map_err(|_| Erro::Adulterado)?;
    let kdf = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let norm = Zeroizing::new(frase.nfc().collect::<String>());
    let mut out = Zeroizing::new([0u8; 32]);
    kdf.hash_password_into(norm.as_bytes(), sal, &mut *out)
        .map_err(|_| Erro::Adulterado)?;
    Ok(out)
}

fn aad_blob(id: &IdArquivo) -> Vec<u8> {
    let mut a = Vec::with_capacity(id.0.len() + 1);
    a.extend_from_slice(id.0.as_bytes());
    a.push(VERSAO_BLOB);
    a
}

impl ChaveCofre {
    /// Chave nova, aleatória (OsRng).
    pub fn nova() -> Self {
        let mut k = [0u8; 32];
        OsRng.fill_bytes(&mut k);
        ChaveCofre(k)
    }

    /// Reconstrói K a partir dos 32 bytes guardados no cofre do sistema.
    /// Tamanho diferente de 32 vira `Erro::Adulterado`.
    pub fn de_bytes(bytes: &[u8]) -> Result<Self, Erro> {
        let k: [u8; 32] = bytes.try_into().map_err(|_| Erro::Adulterado)?;
        Ok(ChaveCofre(k))
    }

    /// Os 32 bytes de K, para guardar no cofre do sistema (Credential Manager, Keystore).
    pub fn bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.0)
    }

    fn subchave(&self, rotulo: &[u8]) -> Zeroizing<[u8; 32]> {
        let hk = Hkdf::<Sha256>::new(None, &self.0);
        let mut out = Zeroizing::new([0u8; 32]);
        hk.expand(rotulo, &mut *out)
            .expect("32 bytes cabem no HKDF-SHA256");
        out
    }

    /// Cifra K com a frase e devolve o conteúdo do `cofre.chave`.
    pub fn embrulhar(&self, frase: &str) -> Result<Vec<u8>, Erro> {
        self.embrulhar_com(frase, ParamsKdf::PADRAO, sal_aleatorio(), nonce_aleatorio())
    }

    pub(crate) fn embrulhar_com(
        &self,
        frase: &str,
        p: ParamsKdf,
        sal: [u8; TAM_SAL],
        nonce: [u8; TAM_NONCE],
    ) -> Result<Vec<u8>, Erro> {
        let mut saida = Vec::with_capacity(TAM_COFRE);
        saida.push(VERSAO_COFRE);
        saida.extend_from_slice(&p.m_kib.to_le_bytes());
        saida.extend_from_slice(&p.t.to_le_bytes());
        saida.extend_from_slice(&p.p.to_le_bytes());
        saida.extend_from_slice(&sal);
        let kf = derivar_frase(frase, &sal, p)?;
        let ct = aead(&kf)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &self.0,
                    aad: &saida,
                },
            )
            .map_err(|_| Erro::Adulterado)?;
        saida.extend_from_slice(&nonce);
        saida.extend_from_slice(&ct);
        Ok(saida)
    }

    /// Abre um `cofre.chave`. Frase errada vira `Erro::FraseErrada`; arquivo truncado ou
    /// com parâmetros trocados vira `Erro::Adulterado` (sem panic).
    pub fn desembrulhar(dados: &[u8], frase: &str) -> Result<Self, Erro> {
        let versao = *dados.first().ok_or(Erro::Adulterado)?;
        if versao != VERSAO_COFRE {
            return Err(Erro::FormatoDesconhecido(versao));
        }
        if dados.len() != TAM_COFRE {
            return Err(Erro::Adulterado);
        }
        let p = ParamsKdf {
            m_kib: ler_u32(&dados[1..5]),
            t: ler_u32(&dados[5..9]),
            p: ler_u32(&dados[9..13]),
        };
        let sal = &dados[13..13 + TAM_SAL];
        let nonce = &dados[TAM_CABECALHO_COFRE..TAM_CABECALHO_COFRE + TAM_NONCE];
        let ct = &dados[TAM_CABECALHO_COFRE + TAM_NONCE..];
        let kf = derivar_frase(frase, sal, p)?;
        let claro = aead(&kf)
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ct,
                    aad: &dados[..TAM_CABECALHO_COFRE],
                },
            )
            .map_err(|_| Erro::FraseErrada)?;
        let claro = Zeroizing::new(claro);
        Self::de_bytes(&claro)
    }

    /// Id do arquivo: HMAC-SHA256(k_nome, caminho normalizado) em hex minúsculo.
    /// Devolve `Result` porque o caminho passa por `normalizar` (que pode recusar).
    pub fn id_de(&self, caminho: &str) -> Result<IdArquivo, Erro> {
        Ok(self.id_normalizado(&normalizar(caminho)?))
    }

    fn id_normalizado(&self, normalizado: &str) -> IdArquivo {
        let kn = self.subchave(ROTULO_NOME);
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&*kn)
            .expect("HMAC aceita qualquer tamanho de chave");
        mac.update(normalizado.as_bytes());
        IdArquivo(hex(&mac.finalize().into_bytes()))
    }

    /// Cifra um arquivo. O id sai de `id_de(caminho)`; o blob não revela o caminho.
    pub fn cifrar(&self, caminho: &str, conteudo: &[u8]) -> Result<Vec<u8>, Erro> {
        self.cifrar_com_nonce(caminho, conteudo, nonce_aleatorio())
    }

    pub(crate) fn cifrar_com_nonce(
        &self,
        caminho: &str,
        conteudo: &[u8],
        nonce: [u8; TAM_NONCE],
    ) -> Result<Vec<u8>, Erro> {
        if conteudo.len() > TAMANHO_MAXIMO {
            return Err(Erro::TamanhoExcedido);
        }
        let caminho = normalizar(caminho)?;
        let id = self.id_normalizado(&caminho);
        let mut texto = Zeroizing::new(Vec::with_capacity(4 + caminho.len() + conteudo.len()));
        texto.extend_from_slice(&(caminho.len() as u32).to_le_bytes());
        texto.extend_from_slice(caminho.as_bytes());
        texto.extend_from_slice(conteudo);
        let kc = self.subchave(ROTULO_CONTEUDO);
        let ct = aead(&kc)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &texto,
                    aad: &aad_blob(&id),
                },
            )
            .map_err(|_| Erro::Adulterado)?;
        let mut blob = Vec::with_capacity(1 + TAM_NONCE + ct.len());
        blob.push(VERSAO_BLOB);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        Ok(blob)
    }

    /// Abre um blob pedido pelo `id` e devolve `(caminho, conteúdo)`. Blob de outro id, byte
    /// trocado ou chave errada viram `Erro::Adulterado`.
    pub fn decifrar(&self, id: &IdArquivo, blob: &[u8]) -> Result<(String, Vec<u8>), Erro> {
        let versao = *blob.first().ok_or(Erro::Adulterado)?;
        if versao != VERSAO_BLOB {
            return Err(Erro::FormatoDesconhecido(versao));
        }
        if blob.len() > TAM_BLOB_MAXIMO {
            return Err(Erro::TamanhoExcedido);
        }
        if blob.len() < TAM_BLOB_MINIMO {
            return Err(Erro::Adulterado);
        }
        let nonce = &blob[1..1 + TAM_NONCE];
        let kc = self.subchave(ROTULO_CONTEUDO);
        let claro = aead(&kc)
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: &blob[1 + TAM_NONCE..],
                    aad: &aad_blob(id),
                },
            )
            .map_err(|_| Erro::Adulterado)?;
        let claro = Zeroizing::new(claro);
        let tam = ler_u32(&claro[..4]) as usize;
        if tam > claro.len() - 4 {
            return Err(Erro::Adulterado);
        }
        let caminho = std::str::from_utf8(&claro[4..4 + tam])
            .map_err(|_| Erro::Adulterado)?
            .to_string();
        // Defesa em profundidade: o caminho de dentro tem de ter o id pedido.
        if self.id_normalizado(&caminho) != *id {
            return Err(Erro::Adulterado);
        }
        Ok((caminho, claro[4 + tam..].to_vec()))
    }
}

#[cfg(test)]
mod testes;
