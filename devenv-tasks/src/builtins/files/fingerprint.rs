use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{self as unix_fs, AtFlags, FileType, Mode, OFlags},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read, Seek},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::Path,
};
use tokio_util::sync::CancellationToken;

#[cfg(test)]
std::thread_local! {
    static DESCRIPTOR_TREE_WALK_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Fingerprint {
    device: u64,
    inode: u64,
    kind: FileKind,
    len: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tree: Option<TreeFingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FileKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct TreeFingerprint {
    entries: u64,
    digest: [u8; 32],
}

// benchmark_staged_fingerprint_reuse: a second walk of 4096 APFS entries
// costs ~69 ms versus ~12 µs to refresh the root. Accumulate metadata while
// copying to avoid that extra traversal; the descriptor walk validates reuse.
pub(super) struct CopiedTreeFingerprintBuilder {
    entries: Vec<CopiedTreeEntry>,
}

struct CopiedTreeEntry {
    relative: Vec<u8>,
    fingerprint: Option<Fingerprint>,
}

impl CopiedTreeFingerprintBuilder {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Reserves the entry's position before a directory is recursively copied.
    /// This preserves the pre-order used by the ordinary descriptor tree walk,
    /// while allowing the directory's final metadata to be recorded afterward.
    pub(super) fn begin_entry(&mut self, relative: &Path) -> usize {
        let index = self.entries.len();
        self.entries.push(CopiedTreeEntry {
            relative: relative.as_os_str().as_bytes().to_vec(),
            fingerprint: None,
        });
        index
    }

    pub(super) fn finish_entry(
        &mut self,
        index: usize,
        fd: &impl AsFd,
        cancellation: &CancellationToken,
    ) -> io::Result<()> {
        self.entries[index].fingerprint = Some(fingerprint_fd(fd, cancellation)?);
        Ok(())
    }

    pub(super) fn finish(
        self,
        root: &impl AsFd,
        cancellation: &CancellationToken,
    ) -> io::Result<Fingerprint> {
        let mut hasher = blake3::Hasher::new();
        for entry in &self.entries {
            check_cancelled(cancellation)?;
            hasher.update(&(entry.relative.len() as u64).to_le_bytes());
            hasher.update(&entry.relative);
            let fingerprint = entry.fingerprint.as_ref().ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    "copied tree fingerprint entry was not finalized",
                )
            })?;
            hash_metadata(&mut hasher, fingerprint);
        }
        let stat = unix_fs::fstat(root).map_err(io::Error::from)?;
        let mut fingerprint = stat_fingerprint(&stat);
        fingerprint.tree = Some(TreeFingerprint {
            entries: self.entries.len() as u64,
            digest: *hasher.finalize().as_bytes(),
        });
        Ok(fingerprint)
    }
}

#[cfg(test)]
pub(super) fn fingerprint(
    path: &Path,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    fingerprint_impl(path, false, cancellation)
}

pub(super) fn fingerprint_at(
    parent: &OwnedFd,
    name: &std::ffi::OsStr,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    check_cancelled(cancellation)?;
    let stat = unix_fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
    let mut fingerprint = stat_fingerprint(&stat);
    if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
        #[cfg(test)]
        record_descriptor_tree_walk();
        let directory = unix_fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        let mut hasher = blake3::Hasher::new();
        let mut entries = 0;
        fingerprint_directory_fd(
            &directory,
            Path::new(""),
            &mut hasher,
            &mut entries,
            cancellation,
        )?;
        fingerprint.tree = Some(TreeFingerprint {
            entries,
            digest: *hasher.finalize().as_bytes(),
        });
    }
    with_content(fingerprint, || open_at(parent, name), cancellation)
}

pub(super) fn fingerprint_fd(
    fd: &impl AsFd,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    check_cancelled(cancellation)?;
    let fingerprint = stat_fingerprint(&unix_fs::fstat(fd)?);
    with_content(
        fingerprint,
        || Ok(File::from(rustix::io::dup(fd)?)),
        cancellation,
    )
}

/// Refreshes metadata for the renamed top-level entry while retaining the
/// descendant digest gathered before commit. Renaming does not change any
/// descendant identity or metadata, but may update the root's ctime.
pub(super) fn refresh_root_fd(
    fd: &impl AsFd,
    mut fingerprint: Fingerprint,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    check_cancelled(cancellation)?;
    let tree = fingerprint.tree.take();
    let content = fingerprint.content;
    fingerprint = stat_fingerprint(&unix_fs::fstat(fd)?);
    fingerprint.tree = tree;
    fingerprint.content = content;
    Ok(fingerprint)
}

#[cfg(test)]
fn record_descriptor_tree_walk() {
    DESCRIPTOR_TREE_WALK_COUNT.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(super) fn reset_descriptor_tree_walk_count() {
    DESCRIPTOR_TREE_WALK_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(super) fn descriptor_tree_walk_count() -> usize {
    DESCRIPTOR_TREE_WALK_COUNT.with(std::cell::Cell::get)
}

fn fingerprint_directory_fd(
    directory: &OwnedFd,
    relative: &Path,
    hasher: &mut blake3::Hasher,
    entries: &mut u64,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    check_cancelled(cancellation)?;
    let mut names = Vec::new();
    let mut directory_entries = unix_fs::Dir::read_from(directory).map_err(io::Error::from)?;
    while let Some(entry) = directory_entries.read() {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(io::Error::from)?;
        let bytes = entry.file_name().to_bytes();
        if bytes != b"." && bytes != b".." {
            names.push(std::ffi::OsString::from_vec(bytes.to_vec()));
        }
    }
    names.sort_unstable();
    for name in names {
        check_cancelled(cancellation)?;
        let child_relative = relative.join(&name);
        let bytes = child_relative.as_os_str().as_bytes();
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
        let stat = unix_fs::statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)?;
        let kind = FileType::from_raw_mode(stat.st_mode);
        let record = with_content(
            stat_fingerprint(&stat),
            || open_at(directory, &name),
            cancellation,
        )?;
        hash_metadata(hasher, &record);
        if kind == FileType::Symlink {
            let target =
                unix_fs::readlinkat(directory, &name, Vec::new()).map_err(io::Error::from)?;
            hasher.update(&(target.as_bytes().len() as u64).to_le_bytes());
            hasher.update(target.as_bytes());
        } else if kind == FileType::Directory {
            let child = unix_fs::openat(
                directory,
                &name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(io::Error::from)?;
            fingerprint_directory_fd(&child, &child_relative, hasher, entries, cancellation)?;
        }
        *entries += 1;
    }
    Ok(())
}

fn stat_fingerprint(stat: &unix_fs::Stat) -> Fingerprint {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => FileKind::File,
        FileType::Directory => FileKind::Directory,
        FileType::Symlink => FileKind::Symlink,
        _ => FileKind::Other,
    };
    #[cfg(target_os = "macos")]
    let (modified, changed) = (
        (stat.st_mtime, stat.st_mtime_nsec),
        (stat.st_ctime, stat.st_ctime_nsec),
    );
    #[cfg(target_os = "linux")]
    let (modified, changed) = (
        (stat.st_mtime, stat.st_mtime_nsec as i64),
        (stat.st_ctime, stat.st_ctime_nsec as i64),
    );
    Fingerprint {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
        kind,
        len: stat.st_size as u64,
        mode: stat.st_mode as u32,
        modified,
        changed,
        tree: None,
        content: None,
    }
}

/// Fingerprints the dereferenced view copied by `cp -RL` semantics.
pub(super) fn source_fingerprint(
    path: &Path,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    fingerprint_impl(path, true, cancellation)
}

fn fingerprint_impl(
    path: &Path,
    follow_symlinks: bool,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    check_cancelled(cancellation)?;
    let metadata = if follow_symlinks {
        fs::metadata(path)?
    } else {
        fs::symlink_metadata(path)?
    };
    let tree = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let mut hasher = blake3::Hasher::new();
        let mut entries = 0;
        for entry in ignore::WalkBuilder::new(path)
            .standard_filters(false)
            .follow_links(follow_symlinks)
            .sort_by_file_name(|a, b| a.cmp(b))
            .build()
        {
            check_cancelled(cancellation)?;
            let entry = entry.map_err(io::Error::other)?;
            if entry.depth() == 0 {
                continue;
            }
            let relative = entry.path().strip_prefix(path).map_err(io::Error::other)?;
            let metadata = entry.metadata().map_err(io::Error::other)?;
            let name = relative.as_os_str().as_bytes();
            hasher.update(&(name.len() as u64).to_le_bytes());
            hasher.update(name);
            let record = with_content(
                metadata_fingerprint(&metadata, None),
                || open_source(entry.path()),
                cancellation,
            )?;
            hash_metadata(&mut hasher, &record);
            entries += 1;
        }
        Some(TreeFingerprint {
            entries,
            digest: *hasher.finalize().as_bytes(),
        })
    } else {
        None
    };
    with_content(
        metadata_fingerprint(&metadata, tree),
        || open_source(path),
        cancellation,
    )
}

/// The immutable shortcut is safe only when every symlink followed while
/// copying also resolves inside the immutable store.
pub(super) fn immutable_store_source(
    path: &Path,
    cancellation: &CancellationToken,
) -> io::Result<Option<std::path::PathBuf>> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(Path::new("/nix/store")) {
        return Ok(None);
    }
    for entry in ignore::WalkBuilder::new(&canonical)
        .standard_filters(false)
        .follow_links(true)
        .build()
    {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(io::Error::other)?;
        if entry.path_is_symlink()
            && !fs::canonicalize(entry.path())?.starts_with(Path::new("/nix/store"))
        {
            return Ok(None);
        }
    }
    Ok(Some(canonical))
}

fn metadata_fingerprint(metadata: &fs::Metadata, tree: Option<TreeFingerprint>) -> Fingerprint {
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        FileKind::File
    } else if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
    };
    Fingerprint {
        device: metadata.dev(),
        inode: metadata.ino(),
        kind,
        len: metadata.len(),
        mode: metadata.mode(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        tree,
        content: None,
    }
}

fn open_at(parent: &OwnedFd, name: &std::ffi::OsStr) -> io::Result<File> {
    Ok(File::from(unix_fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOFOLLOW,
        Mode::empty(),
    )?))
}

fn open_source(path: &Path) -> io::Result<File> {
    Ok(File::from(unix_fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?))
}

fn with_content(
    mut fingerprint: Fingerprint,
    open: impl FnOnce() -> io::Result<File>,
    cancellation: &CancellationToken,
) -> io::Result<Fingerprint> {
    // Neither whole-second HFS+ timestamps nor Linux's historical jiffy-based
    // timestamps distinguish every same-size write. Content is required for
    // ownership checks and cache validation; immutable sources skip this walk.
    // See https://docs.kernel.org/filesystems/multigrain-ts.html.
    if fingerprint.kind == FileKind::File {
        let mut file = open()?;
        file.rewind()?;
        let mut hasher = blake3::Hasher::new();
        loop {
            check_cancelled(cancellation)?;
            if io::copy(&mut file.by_ref().take(8 * 1024 * 1024), &mut hasher)? == 0 {
                break;
            }
        }
        fingerprint.content = Some(*hasher.finalize().as_bytes());
    }
    Ok(fingerprint)
}

fn hash_metadata(hasher: &mut blake3::Hasher, fingerprint: &Fingerprint) {
    hasher.update(&fingerprint.device.to_le_bytes());
    hasher.update(&fingerprint.inode.to_le_bytes());
    hasher.update(&[match fingerprint.kind {
        FileKind::File => 0,
        FileKind::Directory => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }]);
    hasher.update(&fingerprint.len.to_le_bytes());
    hasher.update(&fingerprint.mode.to_le_bytes());
    hasher.update(&fingerprint.modified.0.to_le_bytes());
    hasher.update(&fingerprint.modified.1.to_le_bytes());
    hasher.update(&fingerprint.changed.0.to_le_bytes());
    hasher.update(&fingerprint.changed.1.to_le_bytes());
    if let Some(content) = &fingerprint.content {
        hasher.update(content);
    }
}

fn check_cancelled(cancellation: &CancellationToken) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            ErrorKind::Interrupted,
            "file fingerprint cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rapid_same_size_writes_change_fingerprints() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        let token = CancellationToken::new();
        fs::write(&path, [0]).unwrap();
        let mut previous = source_fingerprint(&path, &token).unwrap();
        // No state writes or sleeps between observations: a complete reconcile
        // can hide Linux timestamp collisions by outlasting a kernel tick.
        for value in 1..=255 {
            fs::write(&path, [value]).unwrap();
            let current = source_fingerprint(&path, &token).unwrap();
            assert_ne!(previous, current);
            previous = current;
        }
    }

    #[test]
    fn changes_when_a_tree_entry_changes() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file");
        fs::write(&file, "first").unwrap();
        let cancellation = CancellationToken::new();
        let before = fingerprint(directory.path(), &cancellation).unwrap();

        fs::write(&file, "different length").unwrap();
        let after = fingerprint(directory.path(), &cancellation).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn observes_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = fingerprint(directory.path(), &cancellation).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Interrupted);
    }
}
