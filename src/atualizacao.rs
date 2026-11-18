//! Conferência local de atualizações (R16): `SHA256SUMS` assinado + hash de cada arquivo.
//!
//! Só verificação local, nenhuma rede. A ideia: o release publica uma lista de hashes
//! (`SHA256SUMS`) e uma assinatura ed25519 dela (`SHA256SUMS.sig`) feita com a chave privada do
//! dono; o app embute só a chave pública. Se a assinatura não bate com a chave embutida, ou se
//! o hash de algum arquivo difere da lista, nada é instalado.
//!
//! Este arquivo usa só a biblioteca padrão, `sha2` e `ed25519-dalek`: o `xtask` o inclui por
//! `#[path]`, e assim quem assina e quem confere compartilham o mesmo formato.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::{
    fmt::{self, Write as _},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Lista de hashes, no formato do `sha256sum`: `<hash>␠␠<caminho com />`.
pub const NOME_SOMAS: &str = "SHA256SUMS";
/// A assinatura da lista: uma linha com 128 caracteres hexadecimais.
pub const NOME_ASSINATURA: &str = "SHA256SUMS.sig";
/// Prefixo do texto assinado: uma assinatura deste app não vale para outro uso da mesma chave.
const DOMINIO: &[u8] = b"rustidian-release-v1\n";
pub const PREFIXO_PUBLICA: &str = "rustidian-chave-publica-v1";
pub const PREFIXO_PRIVADA: &str = "rustidian-chave-privada-v1";
/// O que o arquivo de chave pública diz quando o dono ainda não gerou a dele.
const SEM_CHAVE: &str = "NENHUMA";
/// A chave pública que o app embute (arquivo do repositório, trocado por `cargo xtask chave-nova`).
pub const CHAVE_PUBLICA_EMBUTIDA: &str = include_str!("atualizacao/chave-publica.txt");
/// Passa disso a lista é suspeita (uma lista real tem poucas linhas).
const LIMITE_SOMAS: u64 = 4 << 20;

/// Por que a atualização foi recusada. A mensagem (`Display`) é a que o usuário lê.
#[derive(Debug, PartialEq, Eq)]
pub enum Recusa {
    /// Este build não tem chave pública do dono.
    SemChaveEmbutida,
    ChaveInvalida(String),
    SomasAusente,
    AssinaturaAusente,
    AssinaturaMalformada,
    /// A assinatura não bate com a chave embutida (lista editada ou assinada por outra chave).
    AssinaturaInvalida,
    SomasMalformado(String),
    ArquivoAusente(String),
    HashDiferente(String),
    /// Está na pasta mas não na lista assinada.
    ArquivoNaoListado(String),
    /// O download pedido não consta na lista assinada.
    NaoConstaNaLista(String),
    LinkSimbolico(String),
    Leitura(String),
}

impl fmt::Display for Recusa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recusa::SemChaveEmbutida => write!(
                f,
                "Este Rustidian foi compilado sem a chave pública de assinatura, então não há como \
                 conferir a atualização."
            ),
            Recusa::ChaveInvalida(motivo) => {
                write!(f, "A chave pública embutida no app é inválida: {motivo}.")
            }
            Recusa::SomasAusente => write!(
                f,
                "Falta o arquivo {NOME_SOMAS} ao lado da atualização; ela não foi assinada."
            ),
            Recusa::AssinaturaAusente => write!(
                f,
                "Falta o arquivo {NOME_ASSINATURA}; esta atualização não está assinada."
            ),
            Recusa::AssinaturaMalformada => {
                write!(f, "O arquivo {NOME_ASSINATURA} está malformado.")
            }
            Recusa::AssinaturaInvalida => write!(
                f,
                "A assinatura de {NOME_SOMAS} não confere com a chave do Rustidian: a lista foi \
                 alterada ou foi assinada por outra pessoa."
            ),
            Recusa::SomasMalformado(motivo) => write!(f, "{NOME_SOMAS} malformado: {motivo}."),
            Recusa::ArquivoAusente(nome) => write!(
                f,
                "O arquivo {nome} está na lista assinada mas não existe aqui."
            ),
            Recusa::HashDiferente(nome) => write!(
                f,
                "O arquivo {nome} não é o que foi assinado (o hash difere); ele foi trocado ou \
                 baixou corrompido."
            ),
            Recusa::ArquivoNaoListado(nome) => write!(
                f,
                "O arquivo {nome} está na pasta mas não na lista assinada; ele não será instalado."
            ),
            Recusa::NaoConstaNaLista(nome) => {
                write!(f, "O arquivo {nome} não consta em {NOME_SOMAS}.")
            }
            Recusa::LinkSimbolico(nome) => write!(
                f,
                "{nome} é um link simbólico; a conferência não aceita links."
            ),
            Recusa::Leitura(motivo) => write!(f, "Não consegui ler: {motivo}."),
        }
    }
}

impl std::error::Error for Recusa {}

pub fn hex(bytes: &[u8]) -> String {
    let mut saida = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(saida, "{byte:02x}");
    }
    saida
}

pub fn de_hex(texto: &str) -> Option<Vec<u8>> {
    if !texto.len().is_multiple_of(2) || !texto.is_ascii() {
        return None;
    }
    (0..texto.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&texto[i..i + 2], 16).ok())
        .collect()
}

/// A primeira linha que não é vazia nem comentário (`#`).
fn linha_util(texto: &str) -> Option<&str> {
    texto
        .lines()
        .map(str::trim)
        .find(|linha| !linha.is_empty() && !linha.starts_with('#'))
}

/// Lê `rustidian-chave-publica-v1 <64 hex>`. `Ok(None)` quando diz `NENHUMA`.
pub fn ler_chave_publica(texto: &str) -> Result<Option<VerifyingKey>, Recusa> {
    let linha = linha_util(texto).ok_or_else(|| Recusa::ChaveInvalida("arquivo vazio".into()))?;
    let valor = linha
        .strip_prefix(PREFIXO_PUBLICA)
        .map(str::trim)
        .ok_or_else(|| Recusa::ChaveInvalida("formato desconhecido".into()))?;
    if valor == SEM_CHAVE {
        return Ok(None);
    }
    let bytes: [u8; 32] = de_hex(valor)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Recusa::ChaveInvalida("não tem 64 caracteres hexadecimais".into()))?;
    VerifyingKey::from_bytes(&bytes)
        .map(Some)
        .map_err(|_| Recusa::ChaveInvalida("não é um ponto válido da curva".into()))
}

/// A chave em que este app confia; `SemChaveEmbutida` se o dono ainda não gerou a dele.
pub fn chave_embutida() -> Result<VerifyingKey, Recusa> {
    ler_chave_publica(CHAVE_PUBLICA_EMBUTIDA)?.ok_or(Recusa::SemChaveEmbutida)
}

/// O texto do arquivo de chave pública do repositório.
pub fn texto_chave_publica(chave: &VerifyingKey) -> String {
    format!("{PREFIXO_PUBLICA} {}\n", hex(chave.as_bytes()))
}

/// O texto do arquivo de chave privada (a semente de 32 bytes). Quem chama grava com cuidado.
pub fn texto_chave_privada(chave: &SigningKey) -> String {
    format!("{PREFIXO_PRIVADA} {}\n", hex(&chave.to_bytes()))
}

pub fn ler_chave_privada(texto: &str) -> Result<SigningKey, String> {
    let linha = linha_util(texto).ok_or("o arquivo da chave privada está vazio")?;
    let valor = linha
        .strip_prefix(PREFIXO_PRIVADA)
        .map(str::trim)
        .ok_or("o arquivo não é uma chave privada do Rustidian")?;
    let semente: [u8; 32] = de_hex(valor)
        .and_then(|b| b.try_into().ok())
        .ok_or("a chave privada não tem 64 caracteres hexadecimais")?;
    Ok(SigningKey::from_bytes(&semente))
}

fn hash_do_arquivo(caminho: &Path) -> Result<String, Recusa> {
    let leitura = |e: std::io::Error| Recusa::Leitura(format!("{}: {e}", caminho.display()));
    let mut arquivo = fs::File::open(caminho).map_err(leitura)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let lidos = arquivo.read(&mut buffer).map_err(leitura)?;
        if lidos == 0 {
            break;
        }
        hasher.update(&buffer[..lidos]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Um nome da lista: relativo, com `/`, sem `..`, sem unidade (`C:`) e sem barra invertida.
fn nome_seguro(nome: &str) -> bool {
    !nome.is_empty()
        && !nome.contains(['\\', ':', '\0'])
        && nome
            .split('/')
            .all(|parte| !parte.is_empty() && parte != "." && parte != "..")
}

fn caminho_do_nome(pasta: &Path, nome: &str) -> PathBuf {
    nome.split('/')
        .fold(pasta.to_path_buf(), |caminho, parte| caminho.join(parte))
}

/// `(hash, nome)` de cada linha. Rejeita linha fora do formato, hash que não é SHA-256 em
/// minúsculas, caminho perigoso e nome repetido.
pub fn ler_somas(texto: &str) -> Result<Vec<(String, String)>, Recusa> {
    let mal = Recusa::SomasMalformado;
    let mut itens: Vec<(String, String)> = Vec::new();
    for (indice, linha) in texto.lines().enumerate() {
        let numero = indice + 1;
        let (hash, nome) = linha
            .split_once("  ")
            .ok_or_else(|| mal(format!("linha {numero} fora do formato")))?;
        let hash_ok =
            hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hash_ok {
            return Err(mal(format!("hash inválido na linha {numero}")));
        }
        if !nome_seguro(nome) {
            return Err(mal(format!("caminho inválido na linha {numero}")));
        }
        if itens.iter().any(|(_, existente)| existente == nome) {
            return Err(mal(format!("{nome} aparece duas vezes")));
        }
        itens.push((hash.to_owned(), nome.to_owned()));
    }
    if itens.is_empty() {
        return Err(mal("a lista está vazia".into()));
    }
    Ok(itens)
}

fn texto_assinado(somas: &[u8]) -> Vec<u8> {
    let mut texto = DOMINIO.to_vec();
    texto.extend_from_slice(somas);
    texto
}

/// A assinatura (em hexadecimal) de uma lista `SHA256SUMS`.
pub fn assinar_somas(somas: &[u8], chave: &SigningKey) -> String {
    hex(&chave.sign(&texto_assinado(somas)).to_bytes())
}

/// Confere a assinatura em hexadecimal da lista.
pub fn conferir_assinatura(
    somas: &[u8],
    assinatura_hex: &str,
    chave: &VerifyingKey,
) -> Result<(), Recusa> {
    let bytes: [u8; 64] = de_hex(assinatura_hex.trim())
        .and_then(|b| b.try_into().ok())
        .ok_or(Recusa::AssinaturaMalformada)?;
    chave
        .verify_strict(&texto_assinado(somas), &Signature::from_bytes(&bytes))
        .map_err(|_| Recusa::AssinaturaInvalida)
}

fn ler_arquivo_limitado(caminho: &Path) -> Result<Option<Vec<u8>>, Recusa> {
    match fs::metadata(caminho) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Recusa::Leitura(format!("{}: {e}", caminho.display()))),
        Ok(meta) if !meta.is_file() || meta.len() > LIMITE_SOMAS => Err(Recusa::Leitura(format!(
            "{} não é um arquivo de tamanho razoável",
            caminho.display()
        ))),
        Ok(_) => fs::read(caminho)
            .map(Some)
            .map_err(|e| Recusa::Leitura(format!("{}: {e}", caminho.display()))),
    }
}

/// Lê `SHA256SUMS` e `SHA256SUMS.sig` da pasta, confere a assinatura e devolve as linhas.
/// Nada além disso é confiável antes de esta função passar.
pub fn somas_assinadas(
    pasta: &Path,
    chave: &VerifyingKey,
) -> Result<Vec<(String, String)>, Recusa> {
    let somas = ler_arquivo_limitado(&pasta.join(NOME_SOMAS))?.ok_or(Recusa::SomasAusente)?;
    let assinatura =
        ler_arquivo_limitado(&pasta.join(NOME_ASSINATURA))?.ok_or(Recusa::AssinaturaAusente)?;
    let assinatura = String::from_utf8(assinatura).map_err(|_| Recusa::AssinaturaMalformada)?;
    conferir_assinatura(&somas, &assinatura, chave)?;
    let somas = String::from_utf8(somas)
        .map_err(|_| Recusa::SomasMalformado("não é texto UTF-8".into()))?;
    ler_somas(&somas)
}

fn conferir_item(pasta: &Path, hash: &str, nome: &str) -> Result<(), Recusa> {
    let caminho = caminho_do_nome(pasta, nome);
    match fs::symlink_metadata(&caminho) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Recusa::ArquivoAusente(nome.to_owned()));
        }
        Err(e) => return Err(Recusa::Leitura(format!("{}: {e}", caminho.display()))),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Recusa::LinkSimbolico(nome.to_owned()));
        }
        Ok(meta) if !meta.is_file() => return Err(Recusa::ArquivoAusente(nome.to_owned())),
        Ok(_) => {}
    }
    if hash_do_arquivo(&caminho)? != hash {
        return Err(Recusa::HashDiferente(nome.to_owned()));
    }
    Ok(())
}

/// Todos os arquivos da lista assinada existem na pasta e têm o hash certo. Devolve quantos.
pub fn verificar_lista(pasta: &Path, chave: &VerifyingKey) -> Result<usize, Recusa> {
    let itens = somas_assinadas(pasta, chave)?;
    for (hash, nome) in &itens {
        conferir_item(pasta, hash, nome)?;
    }
    Ok(itens.len())
}

/// Um download isolado (`.tar.gz`, `.deb`...): a lista assinada fica na mesma pasta e precisa
/// listá-lo com o hash certo. Os outros arquivos da pasta não importam.
pub fn verificar_arquivo(arquivo: &Path, chave: &VerifyingKey) -> Result<(), Recusa> {
    let pasta = arquivo
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nome = arquivo
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Recusa::Leitura(format!("nome inválido: {}", arquivo.display())))?;
    let itens = somas_assinadas(pasta, chave)?;
    let (hash, nome) = itens
        .iter()
        .find(|(_, listado)| listado == nome)
        .ok_or_else(|| Recusa::NaoConstaNaLista(nome.to_owned()))?;
    conferir_item(pasta, hash, nome)
}

/// Os caminhos (com `/`) de todos os arquivos sob `pasta`, menos a lista e a assinatura.
fn arquivos_da_pasta(pasta: &Path) -> Result<Vec<String>, Recusa> {
    fn andar(pasta: &Path, prefixo: &str, saida: &mut Vec<String>) -> Result<(), Recusa> {
        let leitura = |e: std::io::Error| Recusa::Leitura(format!("{}: {e}", pasta.display()));
        for entrada in fs::read_dir(pasta).map_err(leitura)? {
            let entrada = entrada.map_err(leitura)?;
            let nome = entrada
                .file_name()
                .into_string()
                .map_err(|_| Recusa::Leitura("nome de arquivo que não é Unicode".into()))?;
            let relativo = if prefixo.is_empty() {
                nome
            } else {
                format!("{prefixo}/{nome}")
            };
            let tipo = entrada.file_type().map_err(leitura)?;
            if tipo.is_symlink() {
                return Err(Recusa::LinkSimbolico(relativo));
            } else if tipo.is_dir() {
                andar(&entrada.path(), &relativo, saida)?;
            } else if relativo != NOME_SOMAS && relativo != NOME_ASSINATURA {
                saida.push(relativo);
            }
        }
        Ok(())
    }
    let mut saida = Vec::new();
    andar(pasta, "", &mut saida)?;
    saida.sort();
    Ok(saida)
}

/// Um pacote extraído (o que `--install` copia): além de a lista conferir, TODO arquivo da
/// pasta precisa estar nela, para ninguém plantar um arquivo que o instalador copiaria.
pub fn verificar_pacote(pasta: &Path, chave: &VerifyingKey) -> Result<usize, Recusa> {
    let itens = somas_assinadas(pasta, chave)?;
    for nome in arquivos_da_pasta(pasta)? {
        if !itens.iter().any(|(_, listado)| *listado == nome) {
            return Err(Recusa::ArquivoNaoListado(nome));
        }
    }
    for (hash, nome) in &itens {
        conferir_item(pasta, hash, nome)?;
    }
    Ok(itens.len())
}

/// O que o `--install` faz antes de copiar qualquer coisa. `sem_assinatura` é a flag de
/// desenvolvedor: pula tudo e devolve um aviso para o usuário ver.
pub fn liberar_instalacao(pacote: &Path, sem_assinatura: bool) -> Result<Option<String>, String> {
    if sem_assinatura {
        return Ok(Some(
            "AVISO: --sem-assinatura: a assinatura e os hashes NÃO foram conferidos (modo \
             desenvolvedor)."
                .to_owned(),
        ));
    }
    let recusa = |r: Recusa| {
        format!(
            "Instalação recusada: {r}\nNada foi instalado ou substituído. Se você é \
             desenvolvedor e construiu este pacote, use --sem-assinatura."
        )
    };
    let chave = chave_embutida().map_err(recusa)?;
    verificar_pacote(pacote, &chave).map_err(recusa)?;
    Ok(None)
}

/// Gera o texto de `SHA256SUMS` de todos os arquivos sob `pasta` (menos a lista e a assinatura),
/// em ordem alfabética.
pub fn gerar_somas(pasta: &Path) -> Result<String, Recusa> {
    let mut texto = String::new();
    for nome in arquivos_da_pasta(pasta)? {
        let hash = hash_do_arquivo(&caminho_do_nome(pasta, &nome))?;
        let _ = writeln!(texto, "{hash}  {nome}");
    }
    if texto.is_empty() {
        return Err(Recusa::SomasMalformado("não há arquivos na pasta".into()));
    }
    Ok(texto)
}

/// Escreve `SHA256SUMS` na pasta e, com chave, `SHA256SUMS.sig`. Sem chave, apaga uma
/// assinatura antiga (ela não valeria para a lista nova). Devolve se saiu assinado.
pub fn gravar_somas(pasta: &Path, chave: Option<&SigningKey>) -> Result<bool, String> {
    let somas = gerar_somas(pasta).map_err(|e| e.to_string())?;
    let erro = |e: std::io::Error| e.to_string();
    fs::write(pasta.join(NOME_SOMAS), &somas).map_err(erro)?;
    let caminho = pasta.join(NOME_ASSINATURA);
    match chave {
        Some(chave) => {
            let assinatura = assinar_somas(somas.as_bytes(), chave);
            fs::write(&caminho, format!("{assinatura}\n")).map_err(erro)?;
            Ok(true)
        }
        None => {
            if caminho.exists() {
                fs::remove_file(&caminho).map_err(erro)?;
            }
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chaves de teste: derivadas de um texto fixo, nunca uma chave real.
    fn chave_de_teste(nome: &str) -> SigningKey {
        let semente: [u8; 32] = Sha256::digest(format!("chave-de-teste-r16/{nome}")).into();
        SigningKey::from_bytes(&semente)
    }

    /// Uma pasta de release assinada: dois arquivos, um em subpasta.
    fn pacote_assinado(chave: &SigningKey) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rustidian"), b"binario").unwrap();
        fs::create_dir(dir.path().join("demo-vault")).unwrap();
        fs::write(dir.path().join("demo-vault/Home.md"), b"# oi").unwrap();
        assert!(gravar_somas(dir.path(), Some(chave)).unwrap());
        dir
    }

    #[test]
    fn assinatura_valida_passa() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        let publica = chave.verifying_key();
        assert_eq!(verificar_lista(dir.path(), &publica), Ok(2));
        assert_eq!(verificar_pacote(dir.path(), &publica), Ok(2));
        assert_eq!(
            verificar_arquivo(&dir.path().join("rustidian"), &publica),
            Ok(())
        );
    }

    #[test]
    fn artefato_trocado_e_recusado() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        fs::write(dir.path().join("rustidian"), b"binario trocado").unwrap();
        let publica = chave.verifying_key();
        let esperado = Err(Recusa::HashDiferente("rustidian".into()));
        assert_eq!(verificar_lista(dir.path(), &publica), esperado);
        assert_eq!(verificar_pacote(dir.path(), &publica), esperado);
        assert_eq!(
            verificar_arquivo(&dir.path().join("rustidian"), &publica),
            esperado.map(|_| ())
        );
    }

    #[test]
    fn lista_editada_e_recusada() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        let hash_novo = hex(&Sha256::digest(b"binario trocado"));
        let somas = fs::read_to_string(dir.path().join(NOME_SOMAS)).unwrap();
        let editada: String = somas
            .lines()
            .map(|linha| match linha.split_once("  ") {
                Some((_, "rustidian")) => format!("{hash_novo}  rustidian\n"),
                _ => format!("{linha}\n"),
            })
            .collect();
        fs::write(dir.path().join(NOME_SOMAS), editada).unwrap();
        fs::write(dir.path().join("rustidian"), b"binario trocado").unwrap();
        // O hash agora bate com o arquivo, mas a assinatura era de outra lista.
        assert_eq!(
            verificar_pacote(dir.path(), &chave.verifying_key()),
            Err(Recusa::AssinaturaInvalida)
        );
    }

    #[test]
    fn assinatura_de_outra_chave_e_recusada() {
        let dono = chave_de_teste("dono");
        let intruso = chave_de_teste("intruso");
        let dir = pacote_assinado(&intruso);
        assert_eq!(
            verificar_pacote(dir.path(), &dono.verifying_key()),
            Err(Recusa::AssinaturaInvalida)
        );
    }

    #[test]
    fn arquivo_faltando_e_recusado() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        let publica = chave.verifying_key();
        fs::remove_file(dir.path().join("demo-vault/Home.md")).unwrap();
        assert_eq!(
            verificar_pacote(dir.path(), &publica),
            Err(Recusa::ArquivoAusente("demo-vault/Home.md".into()))
        );
        fs::remove_file(dir.path().join(NOME_ASSINATURA)).unwrap();
        assert_eq!(
            verificar_pacote(dir.path(), &publica),
            Err(Recusa::AssinaturaAusente)
        );
        fs::remove_file(dir.path().join(NOME_SOMAS)).unwrap();
        assert_eq!(
            verificar_pacote(dir.path(), &publica),
            Err(Recusa::SomasAusente)
        );
    }

    #[test]
    fn arquivo_plantado_no_pacote_e_recusado() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        let publica = chave.verifying_key();
        fs::write(dir.path().join("demo-vault/extra.md"), b"plantado").unwrap();
        assert_eq!(
            verificar_pacote(dir.path(), &publica),
            Err(Recusa::ArquivoNaoListado("demo-vault/extra.md".into()))
        );
        // Num download isolado os outros arquivos da pasta não contam.
        assert_eq!(verificar_lista(dir.path(), &publica), Ok(2));
    }

    #[test]
    fn download_fora_da_lista_e_recusado() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        fs::write(dir.path().join("outro.deb"), b"x").unwrap();
        assert_eq!(
            verificar_arquivo(&dir.path().join("outro.deb"), &chave.verifying_key()),
            Err(Recusa::NaoConstaNaLista("outro.deb".into()))
        );
    }

    #[test]
    fn assinatura_malformada_e_recusada() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        fs::write(dir.path().join(NOME_ASSINATURA), "isto não é hexadecimal\n").unwrap();
        assert_eq!(
            verificar_pacote(dir.path(), &chave.verifying_key()),
            Err(Recusa::AssinaturaMalformada)
        );
    }

    #[test]
    fn lista_com_caminho_perigoso_ou_repetido_e_recusada() {
        let hash = "a".repeat(64);
        for ruim in [
            format!("{hash}  ../fora\n"),
            format!("{hash}  /abs\n"),
            format!("{hash}  C:\\x\n"),
            format!("{hash} um-espaco\n"),
            format!("{}  maiuscula\n", "A".repeat(64)),
            format!("{hash}  a\n{hash}  a\n"),
            String::new(),
        ] {
            assert!(
                matches!(ler_somas(&ruim), Err(Recusa::SomasMalformado(_))),
                "{ruim:?}"
            );
        }
        assert_eq!(ler_somas(&format!("{hash}  a/b.txt\n")).unwrap().len(), 1);
    }

    #[test]
    fn sem_assinatura_recusa_e_flag_de_desenvolvedor_libera() {
        let sem = "# c\nrustidian-chave-publica-v1 NENHUMA\n";
        assert_eq!(ler_chave_publica(sem), Ok(None));
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rustidian"), b"x").unwrap();
        assert!(!gravar_somas(dir.path(), None).unwrap());
        // Pacote sem assinatura: recusa, a não ser com a flag.
        let erro = liberar_instalacao(dir.path(), false).unwrap_err();
        assert!(
            erro.contains("recusada") && erro.contains("Nada foi instalado"),
            "{erro}"
        );
        let aviso = liberar_instalacao(dir.path(), true).unwrap();
        assert!(aviso.unwrap().contains("NÃO foram conferidos"));
    }

    #[test]
    fn gravar_sem_chave_apaga_assinatura_antiga() {
        let chave = chave_de_teste("a");
        let dir = pacote_assinado(&chave);
        assert!(dir.path().join(NOME_ASSINATURA).exists());
        assert!(!gravar_somas(dir.path(), None).unwrap());
        assert!(!dir.path().join(NOME_ASSINATURA).exists());
    }

    #[test]
    fn chaves_viajam_em_texto() {
        let chave = chave_de_teste("a");
        let publica = ler_chave_publica(&texto_chave_publica(&chave.verifying_key()))
            .unwrap()
            .unwrap();
        assert_eq!(publica, chave.verifying_key());
        let privada = ler_chave_privada(&texto_chave_privada(&chave)).unwrap();
        assert_eq!(privada.to_bytes(), chave.to_bytes());
        assert!(ler_chave_privada("rustidian-chave-publica-v1 00").is_err());
        assert!(ler_chave_publica("lixo").is_err());
    }

    #[test]
    fn chave_embutida_no_repositorio_e_legivel() {
        // Vale para "NENHUMA" e para uma chave real: o arquivo nunca pode estar quebrado.
        assert!(ler_chave_publica(CHAVE_PUBLICA_EMBUTIDA).is_ok());
    }
}
