//! Native file dialogs and system file-manager actions.
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::{
    io::Read,
    process::{Child, Command, Stdio},
};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    #[default]
    Folder,
}

/// Select an item in Explorer.
#[cfg(windows)]
pub fn reveal(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // `/select,` takes the quoted path as one raw argument; Explorer exits with 1 even when it
    // worked, so the exit status says nothing.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Cannot open the system file manager: {error}"))
}

/// Select an item in its parent using the desktop file-manager interface.
/// https://wiki.freedesktop.org/www/Specifications/file-manager-interface/
#[cfg(not(windows))]
pub fn reveal(path: &Path) -> Result<(), String> {
    const URI_PATH: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC.remove(b'/');
    // A comma is an array separator in dbus-send; encode it with the other URI bytes.
    let uri = format!(
        "file://{}",
        percent_encoding::percent_encode(path.as_os_str().as_encoded_bytes(), URI_PATH,)
    );
    let output = Command::new("dbus-send")
        .args([
            "--session",
            "--type=method_call",
            "--print-reply",
            "--reply-timeout=5000",
            "--dest=org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .output()
        .map_err(|error| format!("Cannot open the system file manager: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Cannot open the system file manager: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Windows: the system's own dialog (`rfd`), shown from a thread so the window keeps drawing.
#[cfg(windows)]
#[derive(Default)]
pub struct Picker {
    pending: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    kind: Kind,
}

#[cfg(windows)]
impl Picker {
    pub fn active(&self) -> bool {
        self.pending.is_some()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn start(&mut self, kind: Kind, initial: Option<&Path>, title: &str) -> Result<(), String> {
        if self.active() {
            return Err("A file browser is already open. Finish or cancel it first.".into());
        }
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(initial) = initial.filter(|path| !path.as_os_str().is_empty()) {
            // A file path starts the dialog in its folder.
            let folder = if initial.is_dir() {
                Some(initial)
            } else {
                initial.parent()
            };
            if let Some(folder) = folder.filter(|folder| folder.is_dir()) {
                dialog = dialog.set_directory(folder);
            }
        }
        if kind == Kind::File {
            dialog = dialog
                .add_filter(
                    "Notes and source code",
                    &[
                        "md", "txt", "rs", "py", "js", "ts", "json", "toml", "yaml", "yml", "html",
                        "css", "c", "cpp", "h", "sh",
                    ],
                )
                .add_filter("All files", &["*"]);
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("file-dialog".into())
            .spawn(move || {
                let chosen = match kind {
                    Kind::Folder => dialog.pick_folder(),
                    Kind::File => dialog.pick_file(),
                };
                let _ = sender.send(chosen);
            })
            .map_err(|error| format!("Cannot open the file browser: {error}"))?;
        self.pending = Some(receiver);
        self.kind = kind;
        Ok(())
    }

    /// None means still waiting; Ok(None) is an explicit user cancellation.
    pub fn poll(&mut self) -> Option<Result<Option<PathBuf>, String>> {
        use std::sync::mpsc::TryRecvError;
        match self.pending.as_ref()?.try_recv() {
            Ok(path) => {
                self.pending = None;
                Some(Ok(path))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err("The file browser failed.".into()))
            }
        }
    }

    /// The dialog cannot be closed from here; its answer is dropped.
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(not(windows))]
#[derive(Default)]
pub struct Picker {
    child: Option<Child>,
    kind: Kind,
}

#[cfg(not(windows))]
impl Picker {
    pub fn active(&self) -> bool {
        self.child.is_some()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn start(&mut self, kind: Kind, initial: Option<&Path>, title: &str) -> Result<(), String> {
        let mut command = Command::new("zenity");
        command.args(["--file-selection", "--title", title]);
        match kind {
            Kind::Folder => {
                command.arg("--directory");
            }
            Kind::File => {
                command.args([
                    "--file-filter=Notes and source code | *.md *.MD *.txt *.TXT *.rs *.py *.js *.ts *.json *.toml *.yaml *.yml *.html *.css *.c *.cpp *.h *.sh",
                    "--file-filter=All files | *",
                ]);
            }
        }
        if let Some(initial) = initial.filter(|path| !path.as_os_str().is_empty()) {
            command.arg("--filename").arg(initial.join(""));
        }
        self.spawn(kind, &mut command)
    }

    fn spawn(&mut self, kind: Kind, command: &mut Command) -> Result<(), String> {
        if self.active() {
            return Err("A file browser is already open. Finish or cancel it first.".into());
        }
        self.child = Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("Cannot open the file browser: {error}"))?,
        );
        self.kind = kind;
        Ok(())
    }

    /// None means still waiting; Ok(None) is an explicit user cancellation.
    pub fn poll(&mut self) -> Option<Result<Option<PathBuf>, String>> {
        let status = match self.child.as_mut()?.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return None,
            Err(error) => {
                self.cancel();
                return Some(Err(format!("Cannot read the file browser: {error}")));
            }
        };
        let mut child = self.child.take()?;
        if status.code() == Some(1) {
            return Some(Ok(None));
        }
        if !status.success() {
            return Some(Err(format!("The file browser failed ({status}).")));
        }
        Some((|| {
            let mut result = String::new();
            child
                .stdout
                .take()
                .ok_or("The file browser returned no path.")?
                .take(16_385)
                .read_to_string(&mut result)
                .map_err(|error| format!("Cannot read the selected path: {error}"))?;
            if result.len() > 16_384 {
                return Err("The selected path is too long.".into());
            }
            let path = result.strip_suffix('\n').unwrap_or(&result);
            if path.is_empty() {
                return Err("The file browser returned an empty path.".into());
            }
            Ok(Some(PathBuf::from(path)))
        })())
    }

    pub fn cancel(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(not(windows))]
impl Drop for Picker {
    fn drop(&mut self) {
        self.cancel();
    }
}

// The tests drive zenity-style child processes (`sh`, `sleep`, /proc): Linux only.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn finished(picker: &mut Picker) -> Result<Option<PathBuf>, String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = picker.poll() {
                assert!(!picker.active());
                return result;
            }
            assert!(Instant::now() < deadline, "chooser failed to finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn chooser_preserves_unicode_spaces_and_a_filename_newline() {
        let mut picker = Picker::default();
        let expected = "/tmp/Notes سلام/ ending space \n";
        picker
            .spawn(
                Kind::File,
                Command::new("sh").args(["-c", "printf '%s\\n' \"$1\"", "picker-test", expected]),
            )
            .unwrap();
        assert_eq!(picker.kind(), Kind::File);
        assert_eq!(finished(&mut picker).unwrap(), Some(expected.into()));
        assert!(picker.poll().is_none());
    }

    #[test]
    fn cancelled_failed_and_empty_choosers_cannot_select_a_path() {
        for (script, cancelled) in [("exit 1", true), ("exit 2", false), ("exit 0", false)] {
            let mut picker = Picker::default();
            picker
                .spawn(Kind::Folder, Command::new("sh").args(["-c", script]))
                .unwrap();
            let result = finished(&mut picker);
            if cancelled {
                assert_eq!(result.unwrap(), None);
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn pending_chooser_is_polled_without_waiting_and_cancel_reaps_it() {
        let mut picker = Picker::default();
        picker
            .spawn(Kind::Folder, Command::new("sleep").arg("30"))
            .unwrap();
        let pid = picker.child.as_ref().unwrap().id();
        assert!(picker.poll().is_none());
        assert!(
            picker
                .spawn(Kind::File, Command::new("sh").args(["-c", "exit 0"]))
                .is_err()
        );
        assert_eq!(picker.child.as_ref().unwrap().id(), pid);
        picker.cancel();
        assert!(!picker.active());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
}
