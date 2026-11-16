//! O índice (SQLite) e os blobs em disco. Nada aqui lê conteúdo de nota: os blobs são
//! bytes opacos, nomeados pelo SHA-256.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nota_sync::protocolo::{
    BLOB_MAXIMO, CHAVE_MAXIMA, Conflito409, ItemRemoto, Mudancas, PAGINA_MUDANCAS, PedidoPublicar,
    RespostaPublicar, hex64, sha256_hex,
};
use rand_core::{OsRng, RngCore};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub struct Banco {
    conn: Mutex<Connection>,
    blobs: PathBuf,
    chave: PathBuf,
}

/// Por que um blob foi recusado.
#[derive(Debug, PartialEq, Eq)]
pub enum ErroBlob {
    Vazio,
    GrandeDemais,
    HashNaoBate,
}

/// Por que o `cofre.chave` foi recusado.
#[derive(Debug, PartialEq, Eq)]
pub enum ErroChave {
    Vazia,
    GrandeDemais,
    JaExiste,
}

/// Por que uma publicação foi recusada (que não seja o 409).
#[derive(Debug, PartialEq, Eq)]
pub enum ErroPublicar {
    IdInvalido,
    HashInvalido,
    BlobDesconhecido,
}

pub enum ResultadoPublicar {
    Ok(RespostaPublicar),
    Conflito(Conflito409),
}

fn sql(e: rusqlite::Error) -> io::Error {
    io::Error::other(format!("sqlite: {e}"))
}

impl Banco {
    pub fn abrir(dados: &Path) -> io::Result<Self> {
        let blobs = dados.join("blobs");
        fs::create_dir_all(&blobs)?;
        let conn = Connection::open(dados.join("servidor.sqlite3")).map_err(sql)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS itens(
                 id TEXT PRIMARY KEY,
                 versao INTEGER NOT NULL,
                 hash_blob TEXT,
                 apagado INTEGER NOT NULL,
                 seq INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS itens_seq ON itens(seq);
             CREATE TABLE IF NOT EXISTS meta(chave TEXT PRIMARY KEY, valor INTEGER NOT NULL);
             INSERT OR IGNORE INTO meta(chave, valor) VALUES ('seq', 0);",
        )
        .map_err(sql)?;
        Ok(Self {
            conn: Mutex::new(conn),
            blobs,
            chave: dados.join("cofre.chave"),
        })
    }

    fn conexao(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Itens com `seq > desde`, em ordem, no máximo uma página.
    pub fn mudancas(&self, desde: u64) -> io::Result<Mudancas> {
        let c = self.conexao();
        let mut st = c
            .prepare(
                "SELECT id, versao, hash_blob, apagado, seq FROM itens
                 WHERE seq > ?1 ORDER BY seq LIMIT ?2",
            )
            .map_err(sql)?;
        let linhas = st
            .query_map(params![desde as i64, (PAGINA_MUDANCAS + 1) as i64], |r| {
                Ok(ItemRemoto {
                    id: r.get(0)?,
                    versao: r.get::<_, i64>(1)? as u64,
                    hash_blob: r.get(2)?,
                    apagado: r.get::<_, i64>(3)? != 0,
                    seq: r.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(sql)?;
        let mut itens = linhas.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        let mais = itens.len() > PAGINA_MUDANCAS;
        itens.truncate(PAGINA_MUDANCAS);
        let seq = itens.last().map_or(desde, |i| i.seq);
        Ok(Mudancas { itens, seq, mais })
    }

    /// O `cofre.chave` (K embrulhada pela frase). O servidor não o abre: são bytes opacos.
    pub fn ler_chave(&self) -> io::Result<Option<Vec<u8>>> {
        match fs::read(&self.chave) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Grava o `cofre.chave` **uma única vez**. O arquivo completo é escrito num temporário
    /// e ligado ao nome final com `hard_link`, que falha se o nome já existe: dois aparelhos
    /// que chegam juntos não se sobrepõem e ninguém lê um arquivo pela metade.
    pub fn gravar_chave(&self, corpo: &[u8]) -> io::Result<Result<(), ErroChave>> {
        if corpo.is_empty() {
            return Ok(Err(ErroChave::Vazia));
        }
        if corpo.len() > CHAVE_MAXIMA {
            return Ok(Err(ErroChave::GrandeDemais));
        }
        if self.chave.exists() {
            return Ok(Err(ErroChave::JaExiste));
        }
        let mut b = [0u8; 8];
        OsRng.fill_bytes(&mut b);
        let temp = self.chave.with_file_name(format!(
            ".chave-tmp-{}",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        ));
        let r = (|| {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            f.write_all(corpo)?;
            f.sync_all()?;
            drop(f);
            fs::hard_link(&temp, &self.chave)
        })();
        let _ = fs::remove_file(&temp); // o temporário é nosso
        match r {
            Ok(()) => Ok(Ok(())),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(Err(ErroChave::JaExiste)),
            Err(e) => Err(e),
        }
    }

    fn caminho_blob(&self, hash: &str) -> Option<PathBuf> {
        hex64(hash).then(|| self.blobs.join(hash))
    }

    pub fn blob_existe(&self, hash: &str) -> bool {
        self.caminho_blob(hash).is_some_and(|p| p.is_file())
    }

    pub fn ler_blob(&self, hash: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(p) = self.caminho_blob(hash) else {
            return Ok(None);
        };
        match fs::read(p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Guarda o blob de forma atômica e devolve o SHA-256 que o servidor calculou.
    /// `esperado`, se vier, tem de bater com o corpo.
    pub fn guardar_blob(
        &self,
        corpo: &[u8],
        esperado: Option<&str>,
    ) -> io::Result<Result<String, ErroBlob>> {
        if corpo.is_empty() {
            return Ok(Err(ErroBlob::Vazio));
        }
        if corpo.len() > BLOB_MAXIMO {
            return Ok(Err(ErroBlob::GrandeDemais));
        }
        let hash = sha256_hex(corpo);
        if esperado.is_some_and(|e| e != hash) {
            return Ok(Err(ErroBlob::HashNaoBate));
        }
        let destino = self.blobs.join(&hash);
        if destino.is_file() {
            return Ok(Ok(hash)); // mesmo conteúdo: idempotente
        }
        let mut b = [0u8; 8];
        OsRng.fill_bytes(&mut b);
        let temp = self.blobs.join(format!(
            ".tmp-{}",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        ));
        let r = (|| {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            f.write_all(corpo)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&temp, &destino)
        })();
        if let Err(e) = r {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        #[cfg(unix)]
        if let Ok(d) = fs::File::open(&self.blobs) {
            let _ = d.sync_all();
        }
        Ok(Ok(hash))
    }

    /// Publica uma versão do item, numa transação: se `versao_base` não é a versão atual,
    /// devolve o 409 com o estado atual e não muda nada.
    pub fn publicar(
        &self,
        id: &str,
        pedido: &PedidoPublicar,
    ) -> io::Result<Result<ResultadoPublicar, ErroPublicar>> {
        if !hex64(id) {
            return Ok(Err(ErroPublicar::IdInvalido));
        }
        let hash = if pedido.apagado {
            None
        } else {
            match pedido.hash_blob.as_deref() {
                Some(h) if hex64(h) => Some(h),
                _ => return Ok(Err(ErroPublicar::HashInvalido)),
            }
        };
        if hash.is_some_and(|h| !self.blob_existe(h)) {
            return Ok(Err(ErroPublicar::BlobDesconhecido));
        }
        let mut c = self.conexao();
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let atual: Option<(i64, Option<String>, i64)> = tx
            .query_row(
                "SELECT versao, hash_blob, apagado FROM itens WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(sql)?;
        let versao_atual = atual.as_ref().map_or(0, |a| a.0 as u64);
        if versao_atual != pedido.versao_base {
            return Ok(Ok(ResultadoPublicar::Conflito(Conflito409 {
                versao_atual,
                hash_blob: atual.as_ref().and_then(|a| a.1.clone()),
                apagado: atual.is_some_and(|a| a.2 != 0),
            })));
        }
        let seq: i64 = tx
            .query_row("SELECT valor FROM meta WHERE chave = 'seq'", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(sql)?
            + 1;
        tx.execute(
            "UPDATE meta SET valor = ?1 WHERE chave = 'seq'",
            params![seq],
        )
        .map_err(sql)?;
        let versao = versao_atual + 1;
        tx.execute(
            "INSERT INTO itens(id, versao, hash_blob, apagado, seq) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET versao = ?2, hash_blob = ?3, apagado = ?4, seq = ?5",
            params![id, versao as i64, hash, i64::from(pedido.apagado), seq],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(Ok(ResultadoPublicar::Ok(RespostaPublicar {
            versao,
            seq: seq as u64,
        })))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn id(n: u8) -> String {
        sha256_hex(&[n])
    }

    fn pedido(base: u64, hash: &str) -> PedidoPublicar {
        PedidoPublicar {
            versao_base: base,
            hash_blob: Some(hash.into()),
            apagado: false,
        }
    }

    #[test]
    fn blob_idempotente_e_confere_hash() {
        let d = tempfile::tempdir().unwrap();
        let b = Banco::abrir(d.path()).unwrap();
        let h = b.guardar_blob(b"abc", None).unwrap().unwrap();
        assert_eq!(b.guardar_blob(b"abc", Some(&h)).unwrap().unwrap(), h);
        assert_eq!(
            b.guardar_blob(b"abc", Some(&sha256_hex(b"x"))).unwrap(),
            Err(ErroBlob::HashNaoBate)
        );
        assert_eq!(b.guardar_blob(b"", None).unwrap(), Err(ErroBlob::Vazio));
        assert_eq!(b.ler_blob(&h).unwrap().unwrap(), b"abc");
        assert!(b.ler_blob("../../x").unwrap().is_none());
        // nenhum temporário sobra
        let n = fs::read_dir(d.path().join("blobs")).unwrap().count();
        assert_eq!(n, 1);
    }

    #[test]
    fn blob_acima_do_limite_e_recusado_sem_gravar() {
        let d = tempfile::tempdir().unwrap();
        let b = Banco::abrir(d.path()).unwrap();
        let grande = vec![7u8; BLOB_MAXIMO + 1];
        assert_eq!(
            b.guardar_blob(&grande, None).unwrap(),
            Err(ErroBlob::GrandeDemais)
        );
        assert_eq!(fs::read_dir(d.path().join("blobs")).unwrap().count(), 0);
        // no limite exato ainda cabe
        let no_limite = vec![7u8; BLOB_MAXIMO];
        assert!(b.guardar_blob(&no_limite, None).unwrap().is_ok());
    }

    #[test]
    fn publicar_e_transacional_e_o_seq_so_sobe() {
        let d = tempfile::tempdir().unwrap();
        let b = Banco::abrir(d.path()).unwrap();
        let h = b.guardar_blob(b"abc", None).unwrap().unwrap();
        let ResultadoPublicar::Ok(r1) = b.publicar(&id(1), &pedido(0, &h)).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!((r1.versao, r1.seq), (1, 1));
        // base velha: 409 e nada muda
        let ResultadoPublicar::Conflito(c) = b.publicar(&id(1), &pedido(0, &h)).unwrap().unwrap()
        else {
            panic!()
        };
        assert_eq!(c.versao_atual, 1);
        assert_eq!(b.mudancas(0).unwrap().itens.len(), 1);
        let lapide = PedidoPublicar {
            versao_base: 1,
            hash_blob: None,
            apagado: true,
        };
        let ResultadoPublicar::Ok(r2) = b.publicar(&id(1), &lapide).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!((r2.versao, r2.seq), (2, 2));
        let m = b.mudancas(1).unwrap();
        assert_eq!(m.itens.len(), 1);
        assert!(m.itens[0].apagado);
        assert!(b.mudancas(2).unwrap().itens.is_empty());
    }

    #[test]
    fn recusa_blob_que_nao_existe_e_ids_estranhos() {
        let d = tempfile::tempdir().unwrap();
        let b = Banco::abrir(d.path()).unwrap();
        assert_eq!(
            b.publicar(&id(1), &pedido(0, &sha256_hex(b"nao existe")))
                .unwrap()
                .err(),
            Some(ErroPublicar::BlobDesconhecido)
        );
        assert_eq!(
            b.publicar("../x", &pedido(0, &sha256_hex(b"x")))
                .unwrap()
                .err(),
            Some(ErroPublicar::IdInvalido)
        );
    }

    #[test]
    fn estado_sobrevive_a_reabrir() {
        let d = tempfile::tempdir().unwrap();
        let h;
        {
            let b = Banco::abrir(d.path()).unwrap();
            h = b.guardar_blob(b"abc", None).unwrap().unwrap();
            b.publicar(&id(1), &pedido(0, &h)).unwrap().unwrap();
        }
        let b = Banco::abrir(d.path()).unwrap();
        assert_eq!(b.mudancas(0).unwrap().itens.len(), 1);
        let ResultadoPublicar::Ok(r) = b.publicar(&id(2), &pedido(0, &h)).unwrap().unwrap() else {
            panic!()
        };
        assert_eq!(r.seq, 2);
    }
}
