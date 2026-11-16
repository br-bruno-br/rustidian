//! What the sync thread tells the window, and the words the status bar uses for it.

use chrono::NaiveDateTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// The thread is opening its state; no cycle has finished yet.
    #[default]
    Starting,
    Syncing,
    Synced,
    /// The server could not be reached. The app works as usual; the cycle is tried again later.
    Offline,
    /// A cycle stopped on something that retrying will not fix by itself.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncStatus {
    pub phase: Phase,
    /// When the last cycle that reached the server finished (local time).
    pub last_sync: Option<NaiveDateTime>,
    /// Files named `... (conflito <device> <date>).md` in the vault.
    pub conflicts: Vec<String>,
    /// What the last cycle left out and why (invalid names, files over 50 MiB, ...).
    pub refused: Vec<String>,
    /// The message behind `Phase::Failed`.
    pub error: Option<String>,
    /// Why the server could not be reached, for `Phase::Offline`.
    pub offline_reason: Option<String>,
    /// `(deletions, files)` of a cycle stopped by the mass-deletion guard.
    pub mass_deletion: Option<(usize, usize)>,
    /// Cycles finished so far.
    pub cycles: u64,
    /// Counts the cycles after which the vault's files were different on disk.
    pub vault_changed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Busy,
    Attention,
    Bad,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub text: String,
    pub tone: Tone,
}

impl SyncStatus {
    /// One short phrase for the status bar. Priority: syncing, error, offline, conflicts, synced.
    pub fn label(&self) -> Label {
        let (text, tone) = match self.phase {
            Phase::Starting | Phase::Syncing => ("Syncing…".to_owned(), Tone::Busy),
            Phase::Failed => ("Sync error".to_owned(), Tone::Bad),
            Phase::Offline => ("Offline".to_owned(), Tone::Attention),
            Phase::Synced if !self.conflicts.is_empty() => (
                format!(
                    "{} conflict{}",
                    self.conflicts.len(),
                    if self.conflicts.len() == 1 { "" } else { "s" }
                ),
                Tone::Attention,
            ),
            Phase::Synced => (
                match self.last_sync {
                    Some(time) => format!("Synced {}", time.format("%H:%M")),
                    None => "Synced".to_owned(),
                },
                Tone::Good,
            ),
        };
        Label { text, tone }
    }

    /// The text of the mass-deletion stop, for the details window.
    pub fn mass_deletion_text(&self) -> Option<String> {
        self.mass_deletion.map(|(deletions, files)| {
            format!(
                "This sync would delete {deletions} of {files} synced files from the server. The vault looks empty or its drive is not mounted, so nothing was sent. If you really deleted them, allow it once."
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(hour: u32, minute: u32) -> Option<NaiveDateTime> {
        NaiveDate::from_ymd_opt(2026, 11, 15)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
    }

    #[test]
    fn labels_follow_the_priority() {
        let mut status = SyncStatus::default();
        assert_eq!(status.label().text, "Syncing…");
        status.phase = Phase::Synced;
        status.last_sync = at(9, 5);
        assert_eq!(status.label().text, "Synced 09:05");
        assert_eq!(status.label().tone, Tone::Good);
        status.conflicts = vec!["a (conflito pc 2026-11-15 0905).md".into()];
        assert_eq!(status.label().text, "1 conflict");
        status
            .conflicts
            .push("b (conflito pc 2026-11-15 0905).md".into());
        assert_eq!(status.label().text, "2 conflicts");
        status.phase = Phase::Offline;
        assert_eq!(status.label().text, "Offline");
        status.phase = Phase::Failed;
        assert_eq!(status.label().text, "Sync error");
        assert_eq!(status.label().tone, Tone::Bad);
        status.phase = Phase::Syncing;
        assert_eq!(status.label().text, "Syncing…");
    }

    #[test]
    fn mass_deletion_text_says_what_was_held_back() {
        let status = SyncStatus {
            mass_deletion: Some((25, 30)),
            ..SyncStatus::default()
        };
        let text = status.mass_deletion_text().unwrap();
        assert!(text.contains("25 of 30") && text.contains("nothing was sent"));
        assert!(SyncStatus::default().mass_deletion_text().is_none());
    }
}
