//! The photo of the index: what `Vault::open` builds from reading every note (its text, aliases
//! and tags), kept in one file so the next open reads that file instead of every note.
//!
//! One file is much faster to read than a hundred thousand: on Windows the cost of opening a
//! file dwarfs the cost of its bytes. The photo is only a cache. A note is reused when its size
//! and modification time match the ones recorded; anything else is read from the vault, and a
//! photo that is missing, damaged, from another version or from another vault is ignored.
//!
//! Layout, little endian: `MAGIC`, `FORMAT`, the app version, the vault root, the entry count,
//! the entries and a hash of everything before it. Each entry is the vault-relative path, the
//! modification time (ns since the epoch), the size, the text, the aliases and the tags.

use crate::vault::Note;
use std::{
    collections::HashMap,
    fs,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAGIC: &[u8; 8] = b"RSTDXFT\0";
/// Bump when the layout, or what `Note::new` derives from the text, changes.
const FORMAT: u32 = 1;
/// A file touched this close to the photo may change again within the clock's resolution
/// without its time or size changing, so it is not trusted: it is read again next time.
const SETTLE: Duration = Duration::from_secs(2);
/// An entry takes at least this many bytes: the bound on how many a file can claim.
const SMALLEST_ENTRY: u64 = 40;

/// Where to keep the photo of a vault's index.
#[derive(Clone, Debug, Default)]
pub enum IndexPhoto {
    /// The app's state folder; written only for vaults big enough to be slow to open.
    #[default]
    Auto,
    /// No photo: read every note, write nothing.
    Off,
    /// This file, written whatever the size of the vault.
    At(PathBuf),
}

/// What the last open did with the photo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhotoReport {
    /// A usable photo was found.
    pub found: bool,
    /// Notes taken from the photo without reading them.
    pub reused: usize,
    /// Notes read from the vault: new, changed, or not trusted yet.
    pub read: usize,
    /// Notes the photo had that are no longer in the vault.
    pub gone: usize,
    /// A new photo was written.
    pub written: bool,
}

/// The size and modification time a note had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    modified: u64,
    size: u64,
}

impl Stamp {
    pub(crate) fn of(metadata: &fs::Metadata) -> Option<Self> {
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            modified: u64::try_from(modified.as_nanos()).ok()?,
            size: metadata.len(),
        })
    }

    /// Whether the file may still change without this stamp changing.
    fn unsettled(&self, taken: SystemTime) -> bool {
        let Ok(since) = taken.duration_since(UNIX_EPOCH) else {
            return true;
        };
        let limit = since.saturating_sub(SETTLE).as_nanos();
        u128::from(self.modified) >= limit
    }
}

/// One note as the photo remembers it.
pub(crate) struct Entry {
    pub stamp: Stamp,
    pub text: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

const K: u64 = 0x517c_c1b7_2722_0a95;

fn mix(mut hash: u64, bytes: &[u8]) -> u64 {
    let (words, remainder) = bytes.as_chunks::<8>();
    for word in words {
        hash = (hash.rotate_left(5) ^ u64::from_le_bytes(*word)).wrapping_mul(K);
    }
    for byte in remainder {
        hash = (hash.rotate_left(5) ^ u64::from(*byte)).wrapping_mul(K);
    }
    (hash.rotate_left(5) ^ bytes.len() as u64).wrapping_mul(K)
}

struct Writer<W: Write> {
    inner: W,
    hash: u64,
}

impl<W: Write> Writer<W> {
    fn bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.hash = mix(self.hash, bytes);
        self.inner.write_all(bytes)
    }
    fn u32(&mut self, value: u32) -> io::Result<()> {
        self.bytes(&value.to_le_bytes())
    }
    fn u64(&mut self, value: u64) -> io::Result<()> {
        self.bytes(&value.to_le_bytes())
    }
    fn string(&mut self, text: &str) -> io::Result<()> {
        self.u64(text.len() as u64)?;
        self.bytes(text.as_bytes())
    }
    fn strings(&mut self, list: &[String]) -> io::Result<()> {
        self.u32(u32::try_from(list.len()).map_err(io::Error::other)?)?;
        list.iter().try_for_each(|text| self.string(text))
    }
}

struct Reader<R: Read> {
    inner: R,
    hash: u64,
    /// Bytes the file still holds before the closing hash; no length may claim more.
    left: u64,
}

impl<R: Read> Reader<R> {
    fn bytes(&mut self, length: u64) -> Option<Vec<u8>> {
        if length > self.left {
            return None;
        }
        self.left -= length;
        let mut bytes = vec![0; usize::try_from(length).ok()?];
        self.inner.read_exact(&mut bytes).ok()?;
        self.hash = mix(self.hash, &bytes);
        Some(bytes)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N as u64)?.try_into().ok()
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
    fn string(&mut self) -> Option<String> {
        let length = self.u64()?;
        String::from_utf8(self.bytes(length)?).ok()
    }
    fn strings(&mut self) -> Option<Vec<String>> {
        let count = self.u32()?;
        // Each string takes 8 bytes of length at least.
        if u64::from(count) * 8 > self.left {
            return None;
        }
        (0..count).map(|_| self.string()).collect()
    }
}

/// The notes a photo holds, by vault-relative path. `None` when there is no photo or it cannot
/// be trusted: a different layout, app version or vault, a cut or altered file.
pub(crate) fn read(location: &Path, root: &Path) -> Option<HashMap<PathBuf, Entry>> {
    let file = fs::File::open(location).ok()?;
    let length = file.metadata().ok()?.len();
    let mut reader = Reader {
        inner: BufReader::with_capacity(1 << 20, file),
        hash: 0,
        left: length.checked_sub(8)?,
    };
    if &reader.array::<8>()? != MAGIC || reader.u32()? != FORMAT {
        return None;
    }
    if reader.string()? != env!("CARGO_PKG_VERSION") || reader.string()? != root.to_str()? {
        return None;
    }
    let count = reader.u64()?;
    if count > reader.left / SMALLEST_ENTRY {
        return None;
    }
    let mut entries = HashMap::with_capacity(usize::try_from(count).ok()?);
    for _ in 0..count {
        let path = PathBuf::from(reader.string()?);
        let stamp = Stamp {
            modified: reader.u64()?,
            size: reader.u64()?,
        };
        let entry = Entry {
            stamp,
            text: reader.string()?,
            aliases: reader.strings()?,
            tags: reader.strings()?,
        };
        entries.insert(path, entry);
    }
    let mut closing = [0; 8];
    reader.inner.read_exact(&mut closing).ok()?;
    // Nothing may follow the hash, and the hash must be the one of what was read.
    let mut extra = [0; 1];
    let ended = matches!(reader.inner.read(&mut extra), Ok(0));
    (reader.left == 0 && ended && u64::from_le_bytes(closing) == reader.hash).then_some(entries)
}

/// Write the photo of `notes` (with the stamps they had when read) to `location`, replacing the
/// previous one in a single step. Notes without a stamp, with a path that is not UTF-8 or
/// touched less than `SETTLE` before `taken` are left out: the next open reads them.
pub(crate) fn write(
    location: &Path,
    root: &Path,
    taken: SystemTime,
    notes: &[Note],
    stamps: &[Option<Stamp>],
) -> io::Result<()> {
    let root = root
        .to_str()
        .ok_or_else(|| io::Error::other("the vault path is not UTF-8"))?;
    let parent = location
        .parent()
        .ok_or_else(|| io::Error::other("no folder for the index photo"))?;
    let mut folders = fs::DirBuilder::new();
    folders.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        folders.mode(0o700);
    }
    folders.create(parent)?;
    let kept: Vec<(&Note, Stamp, &str)> = notes
        .iter()
        .zip(stamps)
        .filter_map(|(note, stamp)| {
            let stamp = (*stamp)?;
            (!stamp.unsettled(taken)).then_some(())?;
            Some((note, stamp, note.path.to_str()?))
        })
        .collect();
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = Writer {
        inner: BufWriter::with_capacity(1 << 20, temporary),
        hash: 0,
    };
    writer.bytes(MAGIC)?;
    writer.u32(FORMAT)?;
    writer.string(env!("CARGO_PKG_VERSION"))?;
    writer.string(root)?;
    writer.u64(kept.len() as u64)?;
    for (note, stamp, path) in kept {
        writer.string(path)?;
        writer.u64(stamp.modified)?;
        writer.u64(stamp.size)?;
        writer.string(&note.text)?;
        writer.strings(&note.aliases)?;
        writer.strings(&note.tags)?;
    }
    let hash = writer.hash;
    writer.inner.write_all(&hash.to_le_bytes())?;
    let temporary = writer
        .inner
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    temporary.persist(location).map_err(|error| error.error)?;
    Ok(())
}
