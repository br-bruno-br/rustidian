//! Apoio dos testes: aparelhos simulados (cofre e estado em pastas temporárias) contra um
//! `RemotoMemoria` compartilhado.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate, NaiveDateTime};
use nota_cripto::ChaveCofre;
use nota_sync::cofre;
use nota_sync::{ErroSync, Opcoes, Relatorio, RemotoMemoria, Sincronizador};

pub fn clone_chave(k: &ChaveCofre) -> ChaveCofre {
    ChaveCofre::de_bytes(&k.bytes()[..]).unwrap()
}

/// Relógio que anda 1 minuto a cada leitura, para os nomes de conflito variarem.
pub fn relogio(contador: Arc<AtomicU64>) -> impl FnMut() -> NaiveDateTime + Send + 'static {
    move || {
        let n = contador.fetch_add(1, Ordering::SeqCst);
        NaiveDate::from_ymd_opt(2026, 11, 15)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
            + Duration::minutes(n as i64)
    }
}

pub struct Aparelho {
    pub nome: String,
    pub cofre: tempfile::TempDir,
    pub estado: tempfile::TempDir,
    pub sinc: Sincronizador<RemotoMemoria>,
    central: RemotoMemoria,
    chave: ChaveCofre,
    tempo: Arc<AtomicU64>,
}

impl Aparelho {
    pub fn novo(
        nome: &str,
        central: &RemotoMemoria,
        chave: &ChaveCofre,
        tempo: Arc<AtomicU64>,
    ) -> Self {
        let cofre = tempfile::tempdir().unwrap();
        let estado = tempfile::tempdir().unwrap();
        let sinc = Self::montar(nome, cofre.path(), estado.path(), central, chave, &tempo);
        Self {
            nome: nome.into(),
            cofre,
            estado,
            sinc,
            central: central.outro_aparelho(),
            chave: clone_chave(chave),
            tempo,
        }
    }

    fn montar(
        nome: &str,
        cofre: &Path,
        estado: &Path,
        central: &RemotoMemoria,
        chave: &ChaveCofre,
        tempo: &Arc<AtomicU64>,
    ) -> Sincronizador<RemotoMemoria> {
        Sincronizador::novo(
            Opcoes {
                cofre: cofre.to_path_buf(),
                estado: estado.join("sync.redb"),
                aparelho: nome.into(),
            },
            clone_chave(chave),
            central.outro_aparelho(),
        )
        .unwrap()
        .com_relogio(relogio(tempo.clone()))
    }

    /// Fecha e reabre o sincronizador sobre o mesmo cofre e o mesmo estado (como um reinício).
    pub fn reiniciar(&mut self) {
        // Troca por um sincronizador "vazio" para soltar o arquivo do banco antes de reabrir.
        let provisorio = tempfile::tempdir().unwrap();
        let tmp_estado = provisorio.path().join("x.redb");
        let antigo = std::mem::replace(
            &mut self.sinc,
            Sincronizador::novo(
                Opcoes {
                    cofre: provisorio.path().to_path_buf(),
                    estado: tmp_estado,
                    aparelho: self.nome.clone(),
                },
                clone_chave(&self.chave),
                self.central.outro_aparelho(),
            )
            .unwrap(),
        );
        drop(antigo);
        self.sinc = Self::montar(
            &self.nome,
            self.cofre.path(),
            self.estado.path(),
            &self.central,
            &self.chave,
            &self.tempo,
        );
    }

    pub fn caminho(&self, rel: &str) -> PathBuf {
        self.cofre.path().join(rel)
    }

    pub fn escrever(&self, rel: &str, conteudo: &str) {
        let p = self.caminho(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, conteudo).unwrap();
    }

    pub fn ler(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.caminho(rel)).ok()
    }

    /// O usuário apaga pelo app: o arquivo vai para a `.trash/` do cofre.
    pub fn apagar(&self, rel: &str) {
        cofre::mover_para_lixeira(self.cofre.path(), rel, "usuario").unwrap();
    }

    /// Arquivos vivos (fora de `.trash/`, `.rustidian/` etc.): caminho → texto.
    pub fn vivos(&self) -> BTreeMap<String, String> {
        cofre::listar_vivos(self.cofre.path())
            .unwrap()
            .into_iter()
            .map(|(k, v)| (k, String::from_utf8(v).unwrap()))
            .collect()
    }

    /// Todo o texto que está em `.trash/`.
    pub fn lixeira(&self) -> Vec<String> {
        let mut v = Vec::new();
        fn rec(d: &Path, v: &mut Vec<String>) {
            let Ok(rd) = fs::read_dir(d) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    rec(&p, v);
                } else if let Ok(t) = fs::read_to_string(&p) {
                    v.push(t);
                }
            }
        }
        rec(&self.caminho(".trash"), &mut v);
        v
    }

    pub fn conflitos(&self) -> Vec<String> {
        self.vivos()
            .into_keys()
            .filter(|k| k.contains("(conflito "))
            .collect()
    }

    pub fn ciclo(&mut self) -> Result<Relatorio, ErroSync> {
        self.sinc.ciclo()
    }
}

/// Servidor em memória, chave do cofre e relógio compartilhados.
pub struct Mundo {
    pub central: RemotoMemoria,
    pub chave: ChaveCofre,
    pub tempo: Arc<AtomicU64>,
}

impl Mundo {
    pub fn novo() -> Self {
        Self {
            central: RemotoMemoria::novo(),
            chave: ChaveCofre::nova(),
            tempo: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn aparelho(&self, nome: &str) -> Aparelho {
        Aparelho::novo(nome, &self.central, &self.chave, self.tempo.clone())
    }
}
