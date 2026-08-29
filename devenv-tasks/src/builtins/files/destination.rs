//! Destination operations are descriptor-relative so renamed parents and
//! symlink swaps cannot redirect mutations. Path-based traversal/copy helpers
//! cannot provide that guarantee; rustix supplies the portable OS operations.

use super::{
    fingerprint::{self, Fingerprint},
    platform,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, ErrorKind},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags},
};

pub(super) struct Resolver {
    root_path: PathBuf,
    root: OwnedFd,
    // Inputs are sorted, so their parents usually share a long prefix. Keep
    // only the last ancestor chain instead of one descriptor per managed path.
    // benchmark_parent_cache on APFS: 4096 four-level resolutions take ~3 ms
    // with this cache versus ~147 ms reopening the ancestor chain each time.
    parents: Vec<(PathBuf, OwnedFd)>,
}

pub(super) struct Destination {
    path: PathBuf,
    relative_parent: PathBuf,
    parent: OwnedFd,
    name: OsString,
}

pub(super) struct Staged {
    parent: OwnedFd,
    name: OsString,
    kind: FileType,
    device: u64,
    inode: u64,
    artifact: Artifact,
}

/// A private, single-payload staging directory.  The directory is a sibling
/// of the destination, so installing its payload never crosses a mount.
/// Keeping both directory descriptors lets cleanup remain anchored even if
/// the project tree is renamed after installation.
pub(super) struct Artifact {
    relative_parent: PathBuf,
    parent: OwnedFd,
    name: OsString,
    directory: OwnedFd,
    parent_device: u64,
    parent_inode: u64,
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct ArtifactIntent {
    pub(super) relative_parent: PathBuf,
    pub(super) parent_device: u64,
    pub(super) parent_inode: u64,
    pub(super) name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct Deferred {
    pub(super) relative_parent: PathBuf,
    pub(super) parent_device: u64,
    pub(super) parent_inode: u64,
    pub(super) name: String,
    pub(super) device: u64,
    pub(super) inode: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferredRemoval {
    Removed,
    IdentityMismatch,
    Incomplete,
}

pub(super) enum ArtifactIntentResolution {
    Absent,
    Open(Artifact),
    IdentityMismatch,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EntryMetadata {
    pub(super) kind: FileType,
    pub(super) device: u64,
    pub(super) inode: u64,
}

impl Resolver {
    pub(super) fn new(root: &Path) -> io::Result<Self> {
        let root_fd = fs::open(
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        Ok(Self {
            root_path: root.to_path_buf(),
            root: root_fd,
            parents: Vec::new(),
        })
    }

    pub(super) fn destination(
        &mut self,
        relative: &str,
        create_parents: bool,
    ) -> io::Result<Destination> {
        let relative = Path::new(relative);
        let name = relative
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "destination has no name"))?
            .to_os_string();
        let parent_path = relative.parent().unwrap_or_else(|| Path::new(""));
        let parent = self.open_parent(parent_path, create_parents)?;
        Ok(Destination {
            path: self.root_path.join(relative),
            relative_parent: parent_path.to_path_buf(),
            parent,
            name,
        })
    }

    pub(super) fn remove_empty_parents(&mut self, relative: &str) {
        let mut parent = Path::new(relative).parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            let Some(name) = path.file_name() else {
                break;
            };
            let grandparent = path.parent().unwrap_or_else(|| Path::new(""));
            let Ok(directory) = self.open_parent(grandparent, false) else {
                break;
            };
            if fs::unlinkat(&directory, name, AtFlags::REMOVEDIR).is_err() {
                break;
            }
            self.parents.retain(|(cached, _)| !cached.starts_with(path));
            parent = path.parent();
        }
    }

    pub(super) fn remove_deferred_bounded(
        &mut self,
        deferred: &Deferred,
        cancellation: &CancellationToken,
        max_steps: usize,
        max_duration: Duration,
    ) -> io::Result<DeferredRemoval> {
        let parent = match self.open_parent(&deferred.relative_parent, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(DeferredRemoval::IdentityMismatch);
            }
            Err(error) => return Err(error),
        };
        let parent_stat = fs::fstat(&parent).map_err(io::Error::from)?;
        if parent_stat.st_dev as u64 != deferred.parent_device
            || parent_stat.st_ino != deferred.parent_inode
        {
            return Ok(DeferredRemoval::IdentityMismatch);
        }
        let entry = match fs::statat(&parent, &deferred.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(entry) => entry,
            Err(rustix::io::Errno::NOENT) => return Ok(DeferredRemoval::Removed),
            Err(error) => return Err(error.into()),
        };
        if entry.st_dev as u64 != deferred.device || entry.st_ino != deferred.inode {
            return Ok(DeferredRemoval::IdentityMismatch);
        }
        let mut budget = RemovalBudget::bounded(max_steps, max_duration);
        if remove_entry_bounded(
            &parent,
            OsStr::new(&deferred.name),
            cancellation,
            &mut budget,
        )? {
            Ok(DeferredRemoval::Removed)
        } else {
            Ok(DeferredRemoval::Incomplete)
        }
    }

    pub(super) fn open_artifact(&mut self, deferred: &Deferred) -> io::Result<Option<Artifact>> {
        let parent = match self.open_parent(&deferred.relative_parent, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let parent_stat = fs::fstat(&parent).map_err(io::Error::from)?;
        if parent_stat.st_dev as u64 != deferred.parent_device
            || parent_stat.st_ino != deferred.parent_inode
        {
            return Ok(None);
        }
        let stat = match fs::statat(&parent, &deferred.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if stat.st_dev as u64 != deferred.device || stat.st_ino != deferred.inode {
            return Ok(None);
        }
        let directory = fs::openat(
            &parent,
            &deferred.name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        let verified = fs::fstat(&directory).map_err(io::Error::from)?;
        if verified.st_dev as u64 != deferred.device || verified.st_ino != deferred.inode {
            return Ok(None);
        }
        Ok(Some(Artifact {
            relative_parent: deferred.relative_parent.clone(),
            parent,
            name: OsString::from(&deferred.name),
            directory,
            parent_device: deferred.parent_device,
            parent_inode: deferred.parent_inode,
            device: deferred.device,
            inode: deferred.inode,
        }))
    }

    /// Discover private staging directories only in parents that the caller
    /// can justify from current or legacy configuration. This intentionally
    /// does not walk the whole project: a lost state file must not turn an
    /// ordinary reconcile into an unbounded tree scan.
    pub(super) fn discover_orphaned_artifacts(
        &mut self,
        relative_parent: &Path,
        offset: usize,
        cancellation: &CancellationToken,
        max_entries_per_parent: usize,
        max_artifacts: usize,
    ) -> io::Result<(Vec<Deferred>, bool, usize)> {
        let mut artifacts = Vec::new();
        if cancellation.is_cancelled() {
            return Err(io::Error::new(
                ErrorKind::Interrupted,
                "files artifact sweep cancelled",
            ));
        }
        let parent = match self.open_parent(relative_parent, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok((artifacts, true, 0)),
            Err(error) => return Err(error),
        };
        let parent_stat = fs::fstat(&parent).map_err(io::Error::from)?;
        let mut entries = fs::Dir::read_from(&parent).map_err(io::Error::from)?;
        let mut skipped = 0;
        while skipped < offset {
            let Some(entry) = entries.read() else {
                return Ok((artifacts, true, 0));
            };
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                skipped += 1;
            }
        }
        let mut scanned = 0;
        while scanned < max_entries_per_parent && artifacts.len() < max_artifacts {
            if cancellation.is_cancelled() {
                return Err(io::Error::new(
                    ErrorKind::Interrupted,
                    "files artifact sweep cancelled",
                ));
            }
            let Some(entry) = entries.read() else {
                return Ok((artifacts, true, 0));
            };
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name();
            let name_bytes = name.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            scanned += 1;
            let Ok(name) = name.to_str() else {
                continue;
            };
            let Some(id) = name.strip_prefix(".devenv-files-artifact-") else {
                continue;
            };
            if Uuid::parse_str(id).is_err() {
                continue;
            }
            let stat = match fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(rustix::io::Errno::NOENT) => continue,
                Err(error) => return Err(error.into()),
            };
            if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
                || stat.st_mode & 0o077 != 0
                || !is_private_artifact_shape(&parent, name, stat.st_dev as u64, stat.st_ino)?
            {
                continue;
            }
            artifacts.push(Deferred {
                relative_parent: relative_parent.to_path_buf(),
                parent_device: parent_stat.st_dev as u64,
                parent_inode: parent_stat.st_ino,
                name: name.to_owned(),
                device: stat.st_dev as u64,
                inode: stat.st_ino,
            });
        }
        Ok((artifacts, false, offset + scanned))
    }

    pub(super) fn open_artifact_intent(
        &mut self,
        intent: &ArtifactIntent,
    ) -> io::Result<ArtifactIntentResolution> {
        let parent = match self.open_parent(&intent.relative_parent, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(ArtifactIntentResolution::IdentityMismatch);
            }
            Err(error) => return Err(error),
        };
        let parent_stat = fs::fstat(&parent).map_err(io::Error::from)?;
        if parent_stat.st_dev as u64 != intent.parent_device
            || parent_stat.st_ino != intent.parent_inode
        {
            return Ok(ArtifactIntentResolution::IdentityMismatch);
        }
        let stat = match fs::statat(&parent, &intent.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(ArtifactIntentResolution::Absent),
            Err(error) => return Err(error.into()),
        };
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Ok(ArtifactIntentResolution::IdentityMismatch);
        }
        let directory = fs::openat(
            &parent,
            &intent.name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        let verified = fs::fstat(&directory).map_err(io::Error::from)?;
        if verified.st_dev != stat.st_dev || verified.st_ino != stat.st_ino {
            return Ok(ArtifactIntentResolution::IdentityMismatch);
        }
        Ok(ArtifactIntentResolution::Open(Artifact {
            relative_parent: intent.relative_parent.clone(),
            parent,
            name: OsString::from(&intent.name),
            directory,
            parent_device: intent.parent_device,
            parent_inode: intent.parent_inode,
            device: stat.st_dev as u64,
            inode: stat.st_ino,
        }))
    }

    fn open_parent(&mut self, relative: &Path, create: bool) -> io::Result<OwnedFd> {
        if relative.as_os_str().is_empty() {
            return rustix::io::dup(&self.root).map_err(io::Error::from);
        }
        if let Some((_, parent)) = self.parents.iter().find(|(path, _)| path == relative) {
            return rustix::io::dup(parent).map_err(io::Error::from);
        }

        let shared = self
            .parents
            .iter()
            .take_while(|(path, _)| relative.starts_with(path))
            .count();
        self.parents.truncate(shared);
        let component_offset = self
            .parents
            .last()
            .map_or(0, |(path, _)| path.components().count());
        let (mut traversed, mut current) = match self.parents.last() {
            Some((path, parent)) => (
                path.clone(),
                rustix::io::dup(parent).map_err(io::Error::from)?,
            ),
            None => (
                PathBuf::new(),
                rustix::io::dup(&self.root).map_err(io::Error::from)?,
            ),
        };
        for component in relative.components().skip(component_offset) {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "destination parent is not a clean relative path",
                ));
            };
            traversed.push(name);
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
            let next = match fs::openat(&current, name, flags, Mode::empty()) {
                Ok(next) => next,
                Err(rustix::io::Errno::NOENT) if create => {
                    match fs::mkdirat(&current, name, Mode::from_bits_retain(0o777)) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(error.into()),
                    }
                    fs::openat(&current, name, flags, Mode::empty()).map_err(io::Error::from)?
                }
                Err(error) => return Err(error.into()),
            };
            self.parents.push((
                traversed.clone(),
                rustix::io::dup(&next).map_err(io::Error::from)?,
            ));
            current = next;
        }
        Ok(current)
    }
}

impl Destination {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn relative_path(&self) -> PathBuf {
        self.relative_parent.join(&self.name)
    }

    pub(super) fn open(&self) -> io::Result<OwnedFd> {
        fs::openat(
            &self.parent,
            &self.name,
            // A concurrently replaced entry can be a FIFO even after stat.
            // Never block while opening a destination to inspect its contents.
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(io::Error::from)
    }

    pub(super) fn fingerprint(&self, cancellation: &CancellationToken) -> io::Result<Fingerprint> {
        fingerprint::fingerprint_at(&self.parent, &self.name, cancellation)
    }

    pub(super) fn metadata(&self) -> io::Result<Option<EntryMetadata>> {
        match fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(Some(EntryMetadata {
                kind: FileType::from_raw_mode(stat.st_mode),
                device: stat.st_dev as u64,
                inode: stat.st_ino,
            })),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn read_link(&self) -> io::Result<PathBuf> {
        let target =
            fs::readlinkat(&self.parent, &self.name, Vec::new()).map_err(io::Error::from)?;
        Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
    }

    pub(super) fn create_symlink(&self, target: &Path) -> io::Result<()> {
        fs::symlinkat(target, &self.parent, &self.name).map_err(io::Error::from)
    }

    pub(super) fn remove_file(&self) -> io::Result<()> {
        fs::unlinkat(&self.parent, &self.name, AtFlags::empty()).map_err(io::Error::from)
    }

    pub(super) fn create_artifact(&self, name: &OsStr) -> io::Result<Artifact> {
        fs::mkdirat(
            &self.parent,
            name,
            // The parent can further restrict this through umask, but no
            // other account should be able to mutate a live artifact.
            Mode::from_bits_retain(0o700),
        )
        .map_err(io::Error::from)?;
        let result = (|| {
            let directory = fs::openat(
                &self.parent,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(io::Error::from)?;
            let parent = fs::fstat(&self.parent).map_err(io::Error::from)?;
            let stat = fs::fstat(&directory).map_err(io::Error::from)?;
            Ok(Artifact {
                relative_parent: self.relative_parent.clone(),
                parent: rustix::io::dup(&self.parent).map_err(io::Error::from)?,
                name: name.to_os_string(),
                directory,
                parent_device: parent.st_dev as u64,
                parent_inode: parent.st_ino,
                device: stat.st_dev as u64,
                inode: stat.st_ino,
            })
        })();
        if result.is_err() {
            let _ = fs::unlinkat(&self.parent, name, AtFlags::REMOVEDIR);
        }
        result
    }

    pub(super) fn artifact_intent(&self, name: String) -> io::Result<ArtifactIntent> {
        let parent = fs::fstat(&self.parent).map_err(io::Error::from)?;
        Ok(ArtifactIntent {
            relative_parent: self.relative_parent.clone(),
            parent_device: parent.st_dev as u64,
            parent_inode: parent.st_ino,
            name,
        })
    }

    pub(super) fn stage_symlink(&self, artifact: Artifact, target: &Path) -> io::Result<Staged> {
        self.stage(artifact, FileType::Symlink, |parent, name| {
            fs::symlinkat(target, parent, name).map_err(io::Error::from)
        })
        .map(|(staged, ())| staged)
    }

    pub(super) fn stage_file(
        &self,
        artifact: Artifact,
        source: &Path,
        cancellation: &CancellationToken,
    ) -> io::Result<(Staged, File)> {
        self.stage(artifact, FileType::RegularFile, |parent, name| {
            platform::copy_regular_file_at(source, parent, name, cancellation)
        })
    }

    pub(super) fn stage_directory(&self, artifact: Artifact) -> io::Result<(Staged, OwnedFd)> {
        self.stage(artifact, FileType::Directory, |parent, name| {
            fs::mkdirat(parent, name, Mode::from_bits_retain(0o700)).map_err(io::Error::from)?;
            match fs::openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            ) {
                Ok(directory) => Ok(directory),
                Err(error) => {
                    let _ = fs::unlinkat(parent, name, AtFlags::REMOVEDIR);
                    Err(error.into())
                }
            }
        })
    }

    pub(super) fn stage_placeholder(
        &self,
        artifact: Artifact,
        kind: FileType,
    ) -> io::Result<Staged> {
        if kind == FileType::Directory {
            self.stage(artifact, kind, |parent, name| {
                fs::mkdirat(parent, name, Mode::from_bits_retain(0o700)).map_err(io::Error::from)
            })
            .map(|(staged, ())| staged)
        } else {
            self.stage(artifact, kind, |parent, name| {
                fs::openat(
                    parent,
                    name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::CLOEXEC
                        | OFlags::NOFOLLOW,
                    Mode::from_bits_retain(0o600),
                )
                .map(drop)
                .map_err(io::Error::from)
            })
            .map(|(staged, ())| staged)
        }
    }

    fn stage<R>(
        &self,
        artifact: Artifact,
        kind: FileType,
        mut create: impl FnMut(&OwnedFd, &OsStr) -> io::Result<R>,
    ) -> io::Result<(Staged, R)> {
        let name = OsString::from("payload");
        let value = match create(&artifact.directory, &name) {
            Ok(value) => value,
            Err(error) => {
                let _ = artifact.remove(&CancellationToken::new());
                return Err(error);
            }
        };
        let stat = fs::statat(&artifact.directory, &name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)?;
        Ok((
            Staged {
                parent: rustix::io::dup(&artifact.directory).map_err(io::Error::from)?,
                name,
                kind,
                device: stat.st_dev as u64,
                inode: stat.st_ino,
                artifact,
            },
            value,
        ))
    }

    pub(super) fn rename_from(&self, staged: &Staged) -> io::Result<()> {
        match fs::renameat(&staged.parent, &staged.name, &self.parent, &self.name) {
            Ok(()) => Ok(()),
            Err(_error) if self.contains(staged)? => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn rename_to(&self, staged: &mut Staged) -> io::Result<()> {
        let original = self
            .metadata()?
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "destination disappeared"))?;
        match fs::renameat(&self.parent, &self.name, &staged.parent, &staged.name) {
            Ok(()) => {}
            Err(_error) if staged.contains(original.device, original.inode)? => {}
            Err(error) => return Err(error.into()),
        }
        staged.device = original.device;
        staged.inode = original.inode;
        staged.kind = original.kind;
        Ok(())
    }

    pub(super) fn rename_noreplace_from(&self, staged: &Staged) -> io::Result<()> {
        match fs::renameat_with(
            &staged.parent,
            &staged.name,
            &self.parent,
            &self.name,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => return Ok(()),
            Err(_error) if self.contains(staged)? => return Ok(()),
            Err(error) if optional_rename_error(error) => {}
            Err(error) => return Err(error.into()),
        }

        if staged.kind == FileType::Directory {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "atomic no-replace directory rename is unavailable",
            ));
        } else {
            match fs::linkat(
                &staged.parent,
                &staged.name,
                &self.parent,
                &self.name,
                AtFlags::empty(),
            ) {
                Ok(()) => {}
                Err(_error) if self.contains(staged)? => {}
                Err(error) => return Err(error.into()),
            }
            let _ = fs::unlinkat(&staged.parent, &staged.name, AtFlags::empty());
        }
        Ok(())
    }

    pub(super) fn exchange_from(&self, staged: &Staged) -> io::Result<bool> {
        match fs::renameat_with(
            &staged.parent,
            &staged.name,
            &self.parent,
            &self.name,
            RenameFlags::EXCHANGE,
        ) {
            Ok(()) => Ok(true),
            Err(_error) if self.contains(staged)? => Ok(true),
            Err(error) if optional_rename_error(error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn contains(&self, staged: &Staged) -> io::Result<bool> {
        Ok(self.metadata()?.is_some_and(|metadata| {
            metadata.device == staged.device && metadata.inode == staged.inode
        }))
    }
}

impl Staged {
    pub(super) fn is_directory(&self) -> bool {
        self.kind == FileType::Directory
    }

    pub(super) fn remove(self, cancellation: &CancellationToken) -> io::Result<()> {
        self.artifact.remove(cancellation)
    }

    pub(super) fn deferred(&self) -> Deferred {
        // The artifact directory, rather than the former destination name,
        // is the cleanup capability. Its private payload is never reopened
        // through the public project tree.
        self.artifact.deferred()
    }

    pub(super) fn identity(&self) -> EntryMetadata {
        EntryMetadata {
            kind: self.kind,
            device: self.device,
            inode: self.inode,
        }
    }

    pub(super) fn into_artifact(self) -> Artifact {
        self.artifact
    }

    fn contains(&self, device: u64, inode: u64) -> io::Result<bool> {
        Ok(
            match fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(entry) => entry.st_dev as u64 == device && entry.st_ino == inode,
                Err(rustix::io::Errno::NOENT) => false,
                Err(error) => return Err(error.into()),
            },
        )
    }
}

impl Artifact {
    pub(super) fn deferred(&self) -> Deferred {
        Deferred {
            relative_parent: self.relative_parent.clone(),
            parent_device: self.parent_device,
            parent_inode: self.parent_inode,
            name: self.name.to_string_lossy().into_owned(),
            device: self.device,
            inode: self.inode,
        }
    }

    fn remove(self, cancellation: &CancellationToken) -> io::Result<()> {
        match self.remove_bounded(cancellation, usize::MAX, Duration::MAX)? {
            DeferredRemoval::Removed => Ok(()),
            DeferredRemoval::IdentityMismatch => {
                Err(io::Error::other("artifact identity changed before cleanup"))
            }
            DeferredRemoval::Incomplete => unreachable!("unbounded artifact cleanup stopped"),
        }
    }

    pub(super) fn remove_bounded(
        self,
        cancellation: &CancellationToken,
        max_steps: usize,
        max_duration: Duration,
    ) -> io::Result<DeferredRemoval> {
        let parent = fs::fstat(&self.parent).map_err(io::Error::from)?;
        if parent.st_dev as u64 != self.parent_device || parent.st_ino != self.parent_inode {
            return Ok(DeferredRemoval::IdentityMismatch);
        }
        let entry = match fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(entry) => entry,
            Err(rustix::io::Errno::NOENT) => return Ok(DeferredRemoval::Removed),
            Err(error) => return Err(error.into()),
        };
        if entry.st_dev as u64 != self.device || entry.st_ino != self.inode {
            return Ok(DeferredRemoval::IdentityMismatch);
        }

        let mut budget = RemovalBudget::bounded(max_steps, max_duration);
        let mut entries = rustix::fs::Dir::read_from(&self.directory).map_err(io::Error::from)?;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(io::Error::from)?;
            let child = entry.file_name().to_bytes();
            if child == b"." || child == b".." {
                continue;
            }
            if !remove_entry_bounded(
                &self.directory,
                OsStr::from_bytes(child),
                cancellation,
                &mut budget,
            )? {
                return Ok(DeferredRemoval::Incomplete);
            }
        }
        if !budget.consume() {
            return Ok(DeferredRemoval::Incomplete);
        }
        let entry = match fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(entry) => entry,
            Err(rustix::io::Errno::NOENT) => return Ok(DeferredRemoval::Removed),
            Err(error) => return Err(error.into()),
        };
        if entry.st_dev as u64 != self.device || entry.st_ino != self.inode {
            return Ok(DeferredRemoval::IdentityMismatch);
        }
        fs::unlinkat(&self.parent, &self.name, AtFlags::REMOVEDIR).map_err(io::Error::from)?;
        Ok(DeferredRemoval::Removed)
    }

    pub(super) fn payload_identity(&self) -> io::Result<Option<EntryMetadata>> {
        match fs::statat(&self.directory, "payload", AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(Some(EntryMetadata {
                kind: FileType::from_raw_mode(stat.st_mode),
                device: stat.st_dev as u64,
                inode: stat.st_ino,
            })),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn restore_payload_to(
        &self,
        destination: &Destination,
        expected: EntryMetadata,
    ) -> io::Result<bool> {
        let Some(payload) = self.payload_identity()? else {
            return Ok(false);
        };
        if payload.kind != expected.kind
            || payload.device != expected.device
            || payload.inode != expected.inode
        {
            return Ok(false);
        }
        if destination.metadata()?.is_some() {
            return Ok(false);
        }
        fs::renameat(
            &self.directory,
            "payload",
            &destination.parent,
            &destination.name,
        )
        .map_err(io::Error::from)?;
        Ok(true)
    }
}

struct RemovalBudget {
    remaining: Option<usize>,
    started: Instant,
    max_duration: Duration,
}

impl RemovalBudget {
    fn bounded(steps: usize, max_duration: Duration) -> Self {
        Self {
            remaining: Some(steps),
            started: Instant::now(),
            max_duration,
        }
    }

    fn consume(&mut self) -> bool {
        if self.started.elapsed() >= self.max_duration {
            return false;
        }
        let Some(remaining) = &mut self.remaining else {
            return true;
        };
        if *remaining == 0 {
            return false;
        }
        *remaining -= 1;
        true
    }
}

fn is_private_artifact_shape(
    parent: &OwnedFd,
    name: &str,
    expected_device: u64,
    expected_inode: u64,
) -> io::Result<bool> {
    let directory = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let verified = fs::fstat(&directory).map_err(io::Error::from)?;
    if verified.st_dev as u64 != expected_device || verified.st_ino != expected_inode {
        return Ok(false);
    }
    let mut entries = fs::Dir::read_from(&directory).map_err(io::Error::from)?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(io::Error::from)?;
        let child = entry.file_name().to_bytes();
        if child == b"." || child == b".." {
            continue;
        }
        if child != b"payload" {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove_entry_bounded(
    parent: &OwnedFd,
    name: &OsStr,
    cancellation: &CancellationToken,
    budget: &mut RemovalBudget,
) -> io::Result<bool> {
    if cancellation.is_cancelled() {
        return Err(io::Error::new(
            ErrorKind::Interrupted,
            "file cleanup cancelled",
        ));
    }
    let stat = match fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        if !budget.consume() {
            return Ok(false);
        }
        fs::unlinkat(parent, name, AtFlags::empty()).map_err(io::Error::from)?;
        return Ok(true);
    }
    let directory = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let mut entries = rustix::fs::Dir::read_from(&directory).map_err(io::Error::from)?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(io::Error::from)?;
        let child = entry.file_name().to_bytes();
        if child == b"." || child == b".." {
            continue;
        }
        if !remove_entry_bounded(&directory, OsStr::from_bytes(child), cancellation, budget)? {
            return Ok(false);
        }
    }
    if !budget.consume() {
        return Ok(false);
    }
    fs::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(io::Error::from)?;
    Ok(true)
}

fn optional_rename_error(error: rustix::io::Errno) -> bool {
    error == rustix::io::Errno::NOSYS
        || error == rustix::io::Errno::INVAL
        || error == rustix::io::Errno::OPNOTSUPP
        || error == rustix::io::Errno::NOTSUP
        || error == rustix::io::Errno::PERM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual parent descriptor cache benchmark"]
    fn benchmark_parent_cache() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("a/b/c/d")).unwrap();
        for cached in [false, true] {
            let mut resolver = Resolver::new(temp.path()).unwrap();
            let start = Instant::now();
            for _ in 0..4096 {
                if !cached {
                    resolver.parents.clear();
                }
                resolver.destination("a/b/c/d/file", false).unwrap();
            }
            eprintln!(
                "4096 four-level resolutions cached={cached}: {:?}",
                start.elapsed()
            );
        }
    }

    #[test]
    fn staging_and_installation_stay_on_the_resolved_parent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let moved = root.join("moved");
        let source = temp.path().join("source");
        std::fs::create_dir_all(root.join("parent")).unwrap();
        std::fs::write(&source, "contents").unwrap();
        let mut resolver = Resolver::new(&root).unwrap();
        let destination = resolver.destination("parent/file", false).unwrap();

        std::fs::rename(root.join("parent"), &moved).unwrap();
        std::fs::create_dir(root.join("parent")).unwrap();
        let artifact = destination
            .create_artifact(OsStr::new(".devenv-files-artifact-test"))
            .unwrap();
        let (staged, _file) = destination
            .stage_file(artifact, &source, &CancellationToken::new())
            .unwrap();
        destination.rename_noreplace_from(&staged).unwrap();

        assert_eq!(std::fs::read(moved.join("file")).unwrap(), b"contents");
        assert!(!root.join("parent/file").exists());
    }

    #[test]
    fn artifact_cleanup_uses_the_open_parent_after_a_rename() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let moved = root.join("moved");
        let source = temp.path().join("source");
        let artifact_name = ".devenv-files-artifact-test";
        std::fs::create_dir_all(root.join("parent")).unwrap();
        std::fs::write(&source, "contents").unwrap();
        let mut resolver = Resolver::new(&root).unwrap();
        let destination = resolver.destination("parent/file", false).unwrap();
        let artifact = destination
            .create_artifact(OsStr::new(artifact_name))
            .unwrap();
        let (staged, _file) = destination
            .stage_file(artifact, &source, &CancellationToken::new())
            .unwrap();

        std::fs::rename(root.join("parent"), &moved).unwrap();
        std::fs::create_dir(root.join("parent")).unwrap();
        std::fs::create_dir(root.join("parent").join(artifact_name)).unwrap();
        staged.remove(&CancellationToken::new()).unwrap();

        assert!(!moved.join(artifact_name).exists());
        assert!(root.join("parent").join(artifact_name).is_dir());
    }
}
