//! Medição de lentidão, opcional.
//!
//! Com `RUSTIDIAN_PERF=<arquivo>` no ambiente, cada ação que vale medir (abrir o cofre, salvar,
//! buscar, trocar de aba, desenhar um quadro lento) acrescenta ao arquivo uma linha JSON:
//!
//! ```json
//! {"t":1763320000123,"acao":"abrir_cofre","ms":148.2,"notas":10000,"bytes":52428800}
//! ```
//!
//! `t` é o instante em milissegundos desde 1970, `ms` a duração, `notas` e `bytes` o tamanho do
//! cofre (notas indexadas e bytes de texto delas); os dois faltam quando a ação não tem cofre à
//! mão. Sem a variável, ou com ela vazia, nada é aberto nem escrito: a variável é lida uma vez
//! (`OnceLock`) e cada medição desligada custa um teste de ponteiro, sem relógio e sem alocação.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Um quadro que demora mais que isso é registrado como `quadro_lento`.
pub const SLOW_FRAME: Duration = Duration::from_millis(50);

static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn open(path: &Path) -> Option<Mutex<File>> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
        .map(Mutex::new)
}

fn sink() -> Option<&'static Mutex<File>> {
    SINK.get_or_init(|| {
        std::env::var_os("RUSTIDIAN_PERF")
            .filter(|path| !path.is_empty())
            .and_then(|path| open(Path::new(&path)))
    })
    .as_ref()
}

/// Liga a medição para este arquivo, como `RUSTIDIAN_PERF` faria. Só vale se ninguém a leu antes;
/// serve aos testes, que não podem mudar o ambiente do processo.
pub fn enable_at(path: &Path) -> bool {
    SINK.get().is_none() && open(path).is_some_and(|file| SINK.set(Some(file)).is_ok())
}

pub fn enabled() -> bool {
    sink().is_some()
}

/// O tamanho do cofre que acompanha uma medição: notas indexadas e bytes de texto.
pub type Size = (usize, u64);

/// O início de uma medição. Desligada, não guarda relógio nenhum.
#[must_use]
pub struct Timer(Option<Instant>);

pub fn start() -> Timer {
    Timer(enabled().then(Instant::now))
}

impl Timer {
    /// Registra `action` com o tempo decorrido. `size` só é chamada com a medição ligada.
    pub fn finish(self, action: &str, size: impl FnOnce() -> Option<Size>) {
        if let Some(started) = self.0 {
            record(action, started.elapsed(), size());
        }
    }

    /// Como `finish`, mas só registra uma ação que levou ao menos `at_least`.
    pub fn finish_if_over(
        self,
        action: &str,
        at_least: Duration,
        size: impl FnOnce() -> Option<Size>,
    ) {
        if let Some(started) = self.0
            && started.elapsed() >= at_least
        {
            record(action, started.elapsed(), size());
        }
    }
}

/// Escreve uma linha. Falha de escrita é ignorada: medir nunca atrapalha o app.
pub fn record(action: &str, elapsed: Duration, size: Option<Size>) {
    let Some(sink) = sink() else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let mut line = format!(
        "{{\"t\":{now},\"acao\":\"{}\",\"ms\":{:.3}",
        escape(action),
        elapsed.as_secs_f64() * 1000.0
    );
    if let Some((notes, bytes)) = size {
        line.push_str(&format!(",\"notas\":{notes},\"bytes\":{bytes}"));
    }
    line.push_str("}\n");
    let mut file = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = file.write_all(line.as_bytes());
}

fn escape(text: &str) -> String {
    text.chars()
        .flat_map(|character| match character {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            character if character.is_control() => vec!['_'],
            character => vec![character],
        })
        .collect()
}
