//! A nota que está aberta no editor e o salvamento automático.
//!
//! Regra: depois que o dono para de digitar por `ESPERA_SALVAR` a nota é gravada; também
//! grava ao sair da nota e ao ir para segundo plano (quem chama `salvar`). A tela só reflete
//! `estado()`. Nada aqui conhece sinais ou Dioxus, então testa no Windows com um relógio
//! passado pelo teste.
//!
//! R7b-3: o rascunho lembra o hash do arquivo que abriu (ou gravou por último). Se na hora de
//! gravar o arquivo já é outro (o sync trouxe uma versão nova, outro app mexeu), o texto do dono
//! **não pisa nele**: vai para uma cópia de conflito e o rascunho passa a editar essa cópia.

use std::time::{Duration, Instant};

use nota_sync::cofre::hash_de;
use nota_sync::motor::Hash;

use super::cofre::{Cofre, Gravou, explicar};

/// Quanto tempo sem digitar até gravar sozinho.
pub const ESPERA_SALVAR: Duration = Duration::from_millis(1000);

/// O que o indicador da tela mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    Salvo,
    /// Há texto novo ainda não gravado.
    Salvando,
    Erro(String),
}

#[derive(Debug, Clone, Default)]
pub struct Rascunho {
    caminho: Option<String>,
    texto: String,
    sujo: bool,
    ultima_tecla: Option<Instant>,
    erro: Option<String>,
    /// O hash do arquivo como o app o leu ou gravou por último.
    base: Option<Hash>,
    /// Se a gravação desviou para uma cópia de conflito: o caminho dela.
    desvio: Option<String>,
}

impl Rascunho {
    /// Passa a editar `caminho`, que acabou de ser lido com `texto`.
    pub fn abrir(&mut self, caminho: &str, texto: String) {
        *self = Self {
            caminho: Some(caminho.to_string()),
            base: Some(hash_de(texto.as_bytes())),
            texto,
            ..Self::default()
        };
    }

    /// Larga a nota (o texto pendente deve ter sido gravado antes).
    pub fn fechar(&mut self) {
        *self = Self::default();
    }

    pub fn caminho(&self) -> Option<&str> {
        self.caminho.as_deref()
    }

    #[cfg(test)]
    pub fn texto(&self) -> &str {
        &self.texto
    }

    /// A nota agora é outro arquivo (depois de renomear).
    pub fn mudou_de_caminho(&mut self, novo: &str) {
        if self.caminho.is_some() {
            self.caminho = Some(novo.to_string());
        }
    }

    /// A gravação desviou para uma cópia de conflito (a nota mudou por fora)? Caminho da cópia.
    pub fn desvio(&self) -> Option<&str> {
        self.desvio.as_deref()
    }

    /// Sem texto pendente e com o arquivo diferente do que foi lido: a tela deve reler.
    pub fn desatualizada(&self, cofre: &Cofre) -> bool {
        if self.sujo || self.desvio.is_some() {
            return false;
        }
        let Some(caminho) = self.caminho.as_deref() else {
            return false;
        };
        cofre.hash_da_nota(caminho).ok().flatten() != self.base
    }

    /// O dono mexeu no texto.
    pub fn digitou(&mut self, texto: String, agora: Instant) {
        if self.caminho.is_none() {
            return;
        }
        self.texto = texto;
        self.sujo = true;
        self.ultima_tecla = Some(agora);
    }

    pub fn sujo(&self) -> bool {
        self.sujo
    }

    /// Já passou a espera desde a última tecla?
    pub fn vencido(&self, agora: Instant) -> bool {
        self.sujo
            && self
                .ultima_tecla
                .is_some_and(|t| agora.saturating_duration_since(t) >= ESPERA_SALVAR)
    }

    /// Grava agora se houver texto pendente. Em caso de erro o texto continua pendente e o
    /// estado vira `Erro` (a próxima tentativa acontece na próxima espera ou ao sair).
    pub fn salvar(&mut self, cofre: &Cofre) {
        if !self.sujo {
            return;
        }
        let Some(caminho) = self.caminho.clone() else {
            return;
        };
        match cofre.salvar_nota_conferindo(&caminho, &self.texto, self.base) {
            Ok(Gravou::Normal(hash)) => {
                self.base = Some(hash);
                self.sujo = false;
                self.erro = None;
            }
            Ok(Gravou::Desviou { copia, hash }) => {
                // A nota mudou por fora: o texto do dono está salvo na cópia e passa a ser
                // ela que se edita. O arquivo original fica com a versão nova.
                self.caminho = Some(copia.clone());
                self.base = Some(hash);
                self.desvio = Some(copia);
                self.sujo = false;
                self.erro = None;
            }
            Err(e) => {
                self.erro = Some(explicar(&e));
                // Tenta de novo só depois de outra espera, em vez de a cada tique.
                self.ultima_tecla = self.ultima_tecla.map(|_| Instant::now());
            }
        }
    }

    /// Grava se o tempo de espera já passou.
    pub fn salvar_se_vencido(&mut self, cofre: &Cofre, agora: Instant) {
        if self.vencido(agora) {
            self.salvar(cofre);
        }
    }

    pub fn estado(&self) -> Estado {
        match (&self.erro, self.sujo) {
            (Some(e), true) => Estado::Erro(e.clone()),
            (_, true) => Estado::Salvando,
            _ => Estado::Salvo,
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn cofre() -> (tempfile::TempDir, Cofre) {
        let t = tempfile::tempdir().unwrap();
        let c = Cofre::abrir(t.path().join("Cofre")).unwrap();
        (t, c)
    }

    #[test]
    fn so_grava_depois_da_espera() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "antes").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", "antes".into());
        assert_eq!(r.estado(), Estado::Salvo);

        let t0 = Instant::now();
        r.digitou("antes e depois".into(), t0);
        assert_eq!(r.estado(), Estado::Salvando);
        r.salvar_se_vencido(&c, t0 + Duration::from_millis(900));
        assert_eq!(c.ler_nota("n.md").unwrap(), "antes");

        // Continuou digitando: a espera recomeça.
        r.digitou("antes e depois!".into(), t0 + Duration::from_millis(900));
        r.salvar_se_vencido(&c, t0 + Duration::from_millis(1500));
        assert_eq!(c.ler_nota("n.md").unwrap(), "antes");

        r.salvar_se_vencido(&c, t0 + Duration::from_millis(1900));
        assert_eq!(c.ler_nota("n.md").unwrap(), "antes e depois!");
        assert_eq!(r.estado(), Estado::Salvo);
        assert!(!r.sujo());
    }

    #[test]
    fn salvar_ao_sair_nao_espera() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", String::new());
        r.digitou("texto novo".into(), Instant::now());
        r.salvar(&c);
        assert_eq!(c.ler_nota("n.md").unwrap(), "texto novo");
        assert_eq!(r.estado(), Estado::Salvo);
    }

    #[test]
    fn sem_mudanca_nao_regrava() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "igual").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", "igual".into());
        // Alguém mexeu no arquivo por fora: sem digitação, o app não pisa por cima.
        c.salvar_nota("n.md", "mexido por fora").unwrap();
        r.salvar(&c);
        assert_eq!(c.ler_nota("n.md").unwrap(), "mexido por fora");
    }

    #[test]
    fn erro_ao_gravar_mantem_o_texto_pendente() {
        let (t, c) = cofre();
        let mut r = Rascunho::default();
        // Pasta que não existe mais: `escrever_atomico` cria o pai, então use um pai que é arquivo.
        std::fs::write(t.path().join("Cofre").join("arquivo"), "x").unwrap();
        r.abrir("arquivo/n.md", String::new());
        r.digitou("não perder".into(), Instant::now());
        r.salvar(&c);
        assert!(matches!(r.estado(), Estado::Erro(_)));
        assert!(r.sujo());
        assert_eq!(r.texto(), "não perder");
    }

    #[test]
    fn arquivo_mudado_por_fora_nao_e_pisado_e_o_texto_do_dono_vira_copia() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "antes").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", "antes".into());
        r.digitou("antes e a minha frase".into(), Instant::now());
        // O sync trouxe outra versão enquanto o dono digitava.
        c.salvar_nota("n.md", "versão que veio do PC").unwrap();
        r.salvar(&c);
        assert_eq!(c.ler_nota("n.md").unwrap(), "versão que veio do PC");
        let copia = r.desvio().expect("tem de desviar").to_string();
        assert!(copia.starts_with("n (conflito celular "), "{copia}");
        assert_eq!(c.ler_nota(&copia).unwrap(), "antes e a minha frase");
        assert_eq!(r.estado(), Estado::Salvo);
        assert_eq!(r.caminho(), Some(copia.as_str()));
        // Continuar digitando grava na cópia, não no arquivo novo.
        r.digitou(
            "antes e a minha frase, mais um pouco".into(),
            Instant::now(),
        );
        r.salvar(&c);
        assert_eq!(c.ler_nota("n.md").unwrap(), "versão que veio do PC");
        assert_eq!(
            c.ler_nota(&copia).unwrap(),
            "antes e a minha frase, mais um pouco"
        );
    }

    #[test]
    fn mudou_por_fora_para_o_mesmo_texto_nao_e_conflito() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "a").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", "a".into());
        r.digitou("b".into(), Instant::now());
        c.salvar_nota("n.md", "b").unwrap();
        r.salvar(&c);
        assert_eq!(r.desvio(), None);
        assert_eq!(c.listar("").unwrap().len(), 1);
    }

    #[test]
    fn nota_apagada_por_fora_e_recriada_com_o_texto_do_dono() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "a").unwrap();
        let mut r = Rascunho::default();
        r.abrir("n.md", "a".into());
        r.digitou("a, e mais".into(), Instant::now());
        c.para_a_lixeira("n.md").unwrap();
        r.salvar(&c);
        assert_eq!(r.desvio(), None);
        assert_eq!(c.ler_nota("n.md").unwrap(), "a, e mais");
    }

    #[test]
    fn desatualizada_so_sem_texto_pendente() {
        let (_t, c) = cofre();
        c.salvar_nota("n.md", "a").unwrap();
        let mut r = Rascunho::default();
        assert!(!r.desatualizada(&c));
        r.abrir("n.md", "a".into());
        assert!(!r.desatualizada(&c));
        c.salvar_nota("n.md", "b").unwrap();
        assert!(r.desatualizada(&c));
        // Com texto do dono pendente a tela não é relida: o conflito é tratado ao gravar.
        r.digitou("a!".into(), Instant::now());
        assert!(!r.desatualizada(&c));
    }

    #[test]
    fn digitar_sem_nota_aberta_nao_faz_nada() {
        let mut r = Rascunho::default();
        r.digitou("x".into(), Instant::now());
        assert!(!r.sujo());
        assert_eq!(r.caminho(), None);
    }

    #[test]
    fn renomear_troca_o_caminho_sem_perder_o_texto() {
        let mut r = Rascunho::default();
        r.abrir("a.md", "t".into());
        r.mudou_de_caminho("b.md");
        assert_eq!(r.caminho(), Some("b.md"));
        assert_eq!(r.texto(), "t");
    }
}
