//! Onde fica a pasta do cofre.
//!
//! No Android é a pasta do app na área compartilhada, `Android/data/<pacote>/files/Cofre`
//! (a que o gerenciador de arquivos e o Syncthing alcançam, sem pedir permissão de
//! armazenamento amplo). Se o Android não deixar criar lá, cai na pasta interna do app e a
//! tela avisa, para o dono saber que as notas não estão na pasta compartilhada.

use std::fs;
use std::path::{Path, PathBuf};

/// Identificador do app (`Dioxus.toml`).
pub const PACOTE: &str = "dev.rustidian.app";

/// Onde o cofre ficou e, se precisou usar a reserva, o motivo em português.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub raiz: PathBuf,
    pub aviso: Option<String>,
}

/// Caminhos possíveis da pasta externa do app, do mais certo ao menos certo.
pub fn candidatos_externos(pacote: &str, armazenamento: Option<&str>) -> Vec<PathBuf> {
    let mut v = vec![base_externa("/storage/emulated/0", pacote)];
    if let Some(a) = armazenamento.filter(|a| !a.is_empty()) {
        let c = base_externa(a, pacote);
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

fn base_externa(raiz: &str, pacote: &str) -> PathBuf {
    Path::new(raiz.trim_end_matches('/'))
        .join("Android")
        .join("data")
        .join(pacote)
        .join("files")
        .join("Cofre")
}

/// Pasta interna do app (só ele enxerga), usada se a externa falhar.
pub fn reserva_interna(pacote: &str) -> PathBuf {
    PathBuf::from(format!("/data/data/{pacote}/files/Cofre"))
}

/// A pasta existe (ou foi criada) e o app consegue gravar nela?
pub fn utilizavel(pasta: &Path) -> bool {
    if fs::create_dir_all(pasta).is_err() {
        return false;
    }
    // O prefixo `.nota-sync-` e o fim `.tmp` fazem o sincronizador ignorar este arquivo.
    let prova = pasta.join(".nota-sync-prova.tmp");
    let ok = fs::write(&prova, b"ok").is_ok();
    let _ = fs::remove_file(&prova);
    ok
}

/// Escolhe o primeiro candidato utilizável; senão a reserva.
pub fn escolher(candidatos: &[PathBuf], reserva: PathBuf, aviso: &str) -> Local {
    for c in candidatos {
        if utilizavel(c) {
            return Local {
                raiz: c.clone(),
                aviso: None,
            };
        }
    }
    Local {
        raiz: reserva,
        aviso: Some(aviso.to_string()),
    }
}

/// O cofre deste aparelho.
pub fn local_do_cofre() -> Local {
    if cfg!(target_os = "android") {
        let armazenamento = std::env::var("EXTERNAL_STORAGE").ok();
        escolher(
            &candidatos_externos(PACOTE, armazenamento.as_deref()),
            reserva_interna(PACOTE),
            "O Android não deixou criar a pasta Android/data do app. As notas estão na \
             memória interna do app, que o gerenciador de arquivos não enxerga.",
        )
    } else {
        // Fora do Android (testes e prints): `RUSTIDIAN_COFRE` ou uma pasta temporária.
        let raiz = std::env::var_os("RUSTIDIAN_COFRE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("rustidian-android-cofre"));
        Local { raiz, aviso: None }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn caminho_externo_do_pacote() {
        let c = candidatos_externos("dev.rustidian.app", None);
        assert_eq!(
            c,
            [PathBuf::from(
                "/storage/emulated/0/Android/data/dev.rustidian.app/files/Cofre"
            )]
        );
    }

    #[test]
    fn variavel_de_ambiente_vira_segundo_candidato() {
        let c = candidatos_externos("p.q", Some("/sdcard/"));
        assert_eq!(c.len(), 2);
        assert_eq!(c[1], PathBuf::from("/sdcard/Android/data/p.q/files/Cofre"));
        // Igual ao primeiro: não repete. Vazia: ignora.
        assert_eq!(
            candidatos_externos("p.q", Some("/storage/emulated/0")).len(),
            1
        );
        assert_eq!(candidatos_externos("p.q", Some("")).len(), 1);
    }

    #[test]
    fn escolhe_o_primeiro_que_funciona() {
        let t = tempfile::tempdir().unwrap();
        // Um arquivo no lugar da pasta: criar a pasta falha.
        let ruim = t.path().join("arquivo");
        fs::write(&ruim, "x").unwrap();
        let bom = t.path().join("a").join("b");
        let l = escolher(
            &[ruim.join("sub"), bom.clone()],
            t.path().join("reserva"),
            "aviso",
        );
        assert_eq!(l.raiz, bom);
        assert_eq!(l.aviso, None);
        // A prova de escrita não fica para trás.
        assert_eq!(fs::read_dir(&bom).unwrap().count(), 0);
    }

    #[test]
    fn cai_na_reserva_e_avisa() {
        let t = tempfile::tempdir().unwrap();
        let ruim = t.path().join("arquivo");
        fs::write(&ruim, "x").unwrap();
        let reserva = t.path().join("reserva");
        let l = escolher(&[ruim.join("sub")], reserva.clone(), "motivo");
        assert_eq!(l.raiz, reserva);
        assert_eq!(l.aviso.as_deref(), Some("motivo"));
    }

    #[test]
    fn reserva_interna_usa_o_pacote() {
        assert_eq!(
            reserva_interna("dev.rustidian.app"),
            PathBuf::from("/data/data/dev.rustidian.app/files/Cofre")
        );
    }
}
