//! Conectar este aparelho ao servidor: o primeiro aparelho cria a chave do cofre (K) e a coloca,
//! embrulhada pela frase-senha, no servidor (`cofre.chave`); os outros baixam essa chave e a
//! abrem com a mesma frase. É o mesmo desenho do app de mesa (`src/sync/connect.rs`).
//!
//! A frase é só um argumento aqui: nunca é guardada nem registrada. K também não vai para
//! arquivo no Android (ver "Decisão da frase" no PLANO): vive só na memória do app.

use std::fmt;

use nota_cripto::{ChaveCofre, Erro as ErroCripto};
use nota_sync::remoto::{EnvioChave, ErroRemoto, Remoto};

/// Uma chave nova precisa de uma frase que valha o Argon2id que a protege.
pub const MIN_FRASE_NOVA: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resultado {
    /// O servidor não tinha `cofre.chave`: este aparelho criou a chave.
    Criou,
    /// O servidor já tinha e a frase abriu.
    Entrou,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroConexao {
    /// Endereço, nome do aparelho ou token inutilizável (nada foi enviado).
    Ajustes(String),
    /// Frase vazia, ou curta demais para começar um cofre novo.
    Frase(String),
    /// O servidor tem uma chave e esta frase não a abre.
    FraseErrada,
    /// O servidor recusou o token.
    NaoAutorizado,
    /// Não deu para falar com o servidor, ou ele respondeu algo que o app não entende.
    Servidor(String),
    /// O `cofre.chave` do servidor está danificado ou é de uma versão mais nova.
    ChaveDanificada(String),
    /// Falha ao guardar as configurações neste aparelho.
    Local(String),
}

impl fmt::Display for ErroConexao {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErroConexao::Ajustes(m) | ErroConexao::Frase(m) | ErroConexao::Local(m) => {
                write!(f, "{m}")
            }
            ErroConexao::FraseErrada => write!(
                f,
                "Frase-senha incorreta: ela não abre a chave do cofre deste servidor. \
                 Digite a mesma que você escolheu no primeiro aparelho."
            ),
            ErroConexao::NaoAutorizado => write!(
                f,
                "O servidor recusou o token. Confira se é o token deste aparelho."
            ),
            ErroConexao::Servidor(m) => write!(
                f,
                "Não consegui falar com o servidor ({m}). Confira o endereço e se a Tailscale \
                 está ligada neste celular."
            ),
            ErroConexao::ChaveDanificada(m) => write!(
                f,
                "O arquivo da chave do cofre no servidor não pode ser usado: {m}"
            ),
        }
    }
}

impl std::error::Error for ErroConexao {}

fn erro_do_servidor(e: ErroRemoto) -> ErroConexao {
    match e {
        ErroRemoto::NaoAutorizado => ErroConexao::NaoAutorizado,
        outro => ErroConexao::Servidor(outro.to_string()),
    }
}

fn abrir_chave(embrulhada: &[u8], frase: &str) -> Result<ChaveCofre, ErroConexao> {
    ChaveCofre::desembrulhar(embrulhada, frase).map_err(|e| match e {
        ErroCripto::FraseErrada => ErroConexao::FraseErrada,
        outro => ErroConexao::ChaveDanificada(outro.to_string()),
    })
}

/// O miolo de "primeiro aparelho ou não". Só fala com `remoto`; não toca no disco.
pub fn estabelecer_chave<R: Remoto>(
    remoto: &mut R,
    frase: &str,
) -> Result<(ChaveCofre, Resultado), ErroConexao> {
    if frase.is_empty() {
        return Err(ErroConexao::Frase("Digite a frase-senha.".into()));
    }
    if let Some(embrulhada) = remoto.baixar_chave().map_err(erro_do_servidor)? {
        return Ok((abrir_chave(&embrulhada, frase)?, Resultado::Entrou));
    }
    if frase.chars().count() < MIN_FRASE_NOVA {
        return Err(ErroConexao::Frase(format!(
            "O servidor ainda não tem a chave do cofre, então esta frase-senha vai proteger uma \
             nova: use pelo menos {MIN_FRASE_NOVA} caracteres."
        )));
    }
    let chave = ChaveCofre::nova();
    let embrulhada = chave
        .embrulhar(frase)
        .map_err(|e| ErroConexao::ChaveDanificada(e.to_string()))?;
    match remoto.enviar_chave(&embrulhada).map_err(erro_do_servidor)? {
        EnvioChave::Gravada => Ok((chave, Resultado::Criou)),
        // Outro aparelho chegou primeiro (dois primeiros ao mesmo tempo): vale a chave dele.
        EnvioChave::JaExiste => {
            let deles = remoto
                .baixar_chave()
                .map_err(erro_do_servidor)?
                .ok_or_else(|| ErroConexao::Servidor("a chave do cofre sumiu".into()))?;
            Ok((abrir_chave(&deles, frase)?, Resultado::Entrou))
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use nota_sync::RemotoMemoria;

    #[test]
    fn o_primeiro_aparelho_cria_e_o_segundo_entra_com_a_mesma_frase() {
        let mut primeiro = RemotoMemoria::novo();
        let mut segundo = primeiro.outro_aparelho();
        let (k1, r1) = estabelecer_chave(&mut primeiro, "uma frase bem longa").unwrap();
        assert_eq!(r1, Resultado::Criou);
        let (k2, r2) = estabelecer_chave(&mut segundo, "uma frase bem longa").unwrap();
        assert_eq!(r2, Resultado::Entrou);
        assert_eq!(&k1.bytes()[..], &k2.bytes()[..]);
    }

    #[test]
    fn frase_errada_diz_isso_claramente_e_nao_mexe_no_servidor() {
        let mut primeiro = RemotoMemoria::novo();
        let mut segundo = primeiro.outro_aparelho();
        estabelecer_chave(&mut primeiro, "uma frase bem longa").unwrap();
        let antes = segundo.baixar_chave().unwrap();
        let erro = estabelecer_chave(&mut segundo, "outra frase qualquer").unwrap_err();
        assert_eq!(erro, ErroConexao::FraseErrada);
        assert!(erro.to_string().starts_with("Frase-senha incorreta"));
        assert_eq!(segundo.baixar_chave().unwrap(), antes);
    }

    #[test]
    fn frase_vazia_ou_curta_e_recusada_antes_de_enviar_qualquer_chave() {
        let mut remoto = RemotoMemoria::novo();
        assert!(matches!(
            estabelecer_chave(&mut remoto, ""),
            Err(ErroConexao::Frase(_))
        ));
        assert!(matches!(
            estabelecer_chave(&mut remoto, "curta"),
            Err(ErroConexao::Frase(_))
        ));
        assert_eq!(remoto.baixar_chave().unwrap(), None);
    }
}
