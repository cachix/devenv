use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags},
};

const MAX_ORPHAN_TEMP_SCAN_ENTRIES: usize = 256;
const MAX_ORPHAN_TEMP_REMOVALS: usize = 32;

pub(super) struct StateDirectory {
    path: PathBuf,
    parent_path: PathBuf,
    parent: OwnedFd,
    name: OsString,
    lock_name: OsString,
    recovery_name: OsString,
    state_temporary_prefix: OsString,
    recovery_temporary_prefix: OsString,
}

impl StateDirectory {
    pub(super) fn new(path: &Path) -> io::Result<Self> {
        let parent_path = path
            .parent()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "state path has no parent"))?
            .to_path_buf();
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "state path has no name"))?
            .to_os_string();
        let lock_name = path
            .with_extension("lock")
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "lock path has no name"))?
            .to_os_string();
        let recovery_name = path
            .with_extension("recovery")
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "recovery path has no name"))?
            .to_os_string();
        let state_temporary_prefix = scoped_temporary_prefix("state", &name);
        let recovery_temporary_prefix = scoped_temporary_prefix("recovery", &name);
        let parent = fs::open(
            &parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        Ok(Self {
            path: path.to_path_buf(),
            parent_path,
            parent,
            name,
            lock_name,
            recovery_name,
            state_temporary_prefix,
            recovery_temporary_prefix,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn recovery_path(&self) -> PathBuf {
        self.parent_path.join(&self.recovery_name)
    }

    pub(super) fn open_lock(&self) -> io::Result<File> {
        // APFS can return ENOENT when concurrent O_CREAT opens race on an
        // absent file. concurrent_first_opens_share_one_lock_file reproduces
        // this; the parent is already anchored and does not need recreating.
        let mut retries = 3;
        loop {
            match self.open_regular(&self.lock_name, OFlags::RDWR | OFlags::CREATE) {
                Err(error) if error.kind() == ErrorKind::NotFound && retries > 0 => {
                    retries -= 1;
                    std::thread::yield_now();
                }
                result => return result,
            }
        }
    }

    // NONBLOCK lets us reject a FIFO before an open can park the worker.
    // Descriptor-relative operations keep state anchored if its parent moves.
    fn open_regular(&self, name: &OsStr, flags: OFlags) -> io::Result<File> {
        let file = File::from(fs::openat(
            &self.parent,
            name,
            flags | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::from_bits_retain(0o600),
        )?);
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "state entry is not a regular file",
            ));
        }
        Ok(file)
    }

    fn open_optional(&self, name: &OsStr) -> io::Result<Option<File>> {
        match self.open_regular(name, OFlags::RDONLY) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn open_state(&self) -> io::Result<Option<File>> {
        self.open_optional(&self.name)
    }

    pub(super) fn open_recovery(&self) -> io::Result<Option<File>> {
        self.open_optional(&self.recovery_name)
    }

    pub(super) fn append_recovery(&self) -> io::Result<File> {
        self.open_regular(&self.recovery_name, OFlags::WRONLY | OFlags::APPEND)
    }

    pub(super) fn temporary(&self) -> io::Result<(File, OsString)> {
        self.temporary_with_prefix(&self.state_temporary_prefix)
    }

    fn temporary_with_prefix(&self, prefix: &OsStr) -> io::Result<(File, OsString)> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(prefix).disable_cleanup(true);
        let temporary = builder.make_in(&self.parent_path, |candidate| {
            let name = candidate.file_name().ok_or_else(|| {
                io::Error::new(ErrorKind::InvalidInput, "temporary state has no name")
            })?;
            fs::openat(
                &self.parent,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::from_bits_retain(0o600),
            )
            .map(File::from)
            .map_err(io::Error::from)
        })?;
        let (file, path) = temporary.keep().map_err(|error| error.error)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "temporary state has no name"))?
            .to_os_string();
        Ok((file, name))
    }

    pub(super) fn replace(&self, temporary: &OsStr) -> io::Result<()> {
        fs::renameat(&self.parent, temporary, &self.parent, &self.name).map_err(io::Error::from)
    }

    pub(super) fn remove_temporary(&self, temporary: &OsStr) {
        let _ = fs::unlinkat(&self.parent, temporary, fs::AtFlags::empty());
    }

    pub(super) fn recovery_temporary(&self) -> io::Result<(File, OsString)> {
        self.temporary_with_prefix(&self.recovery_temporary_prefix)
    }

    pub(super) fn replace_recovery(&self, temporary: &OsStr) -> io::Result<()> {
        fs::renameat(&self.parent, temporary, &self.parent, &self.recovery_name)
            .map_err(io::Error::from)
    }

    pub(super) fn remove_recovery(&self) -> io::Result<()> {
        match fs::unlinkat(&self.parent, &self.recovery_name, fs::AtFlags::empty()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Remove only stale files made by this state file's atomic-replace
    /// helpers. This is deliberately bounded and descriptor-relative: the
    /// state parent may contain unrelated runner state, and a
    /// sidecar/reconciliation path must never turn it into an unbounded
    /// directory walk.
    pub(super) fn sweep_orphaned_temporaries(&self) -> io::Result<usize> {
        let mut entries = fs::Dir::read_from(&self.parent).map_err(io::Error::from)?;
        let mut scanned = 0;
        let mut removed = 0;
        while scanned < MAX_ORPHAN_TEMP_SCAN_ENTRIES && removed < MAX_ORPHAN_TEMP_REMOVALS {
            let Some(entry) = entries.read() else {
                break;
            };
            let entry = entry.map_err(io::Error::from)?;
            scanned += 1;
            let name = entry.file_name();
            let bytes = name.to_bytes();
            if !(bytes.starts_with(self.state_temporary_prefix.as_bytes())
                || bytes.starts_with(self.recovery_temporary_prefix.as_bytes()))
            {
                continue;
            }
            let stat = match fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(rustix::io::Errno::NOENT) => continue,
                Err(error) => return Err(error.into()),
            };
            // Tempfile creates mode 0600 files owned by this process. Do not
            // follow links, recurse into directories, or touch shared/hard-
            // linked files even if an untrusted entry mimics this state's
            // collision-resistant prefix.
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
                || stat.st_uid != nix::unistd::Uid::current().as_raw()
                || stat.st_nlink != 1
                || stat.st_mode & 0o077 != 0
            {
                continue;
            }
            let opened = match fs::openat(
                &self.parent,
                name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            ) {
                Ok(file) => file,
                Err(rustix::io::Errno::NOENT) => continue,
                Err(error) => return Err(error.into()),
            };
            let verified = fs::fstat(&opened).map_err(io::Error::from)?;
            if verified.st_dev != stat.st_dev
                || verified.st_ino != stat.st_ino
                || FileType::from_raw_mode(verified.st_mode) != FileType::RegularFile
                || verified.st_uid != stat.st_uid
                || verified.st_nlink != 1
                || verified.st_mode & 0o077 != 0
            {
                continue;
            }
            // POSIX has no unlink-by-file-descriptor operation. A hostile
            // same-UID process can still replace this name after the check;
            // the random tempfile suffix and state-scoped prefix protect live
            // files, while the checks above avoid following or deleting the
            // common unsafe entry types.
            match fs::unlinkat(&self.parent, name, AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => removed += 1,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(removed)
    }
}

fn scoped_temporary_prefix(kind: &str, state_name: &OsStr) -> OsString {
    // Keep names filesystem-safe even when the configured state name contains
    // arbitrary Unix bytes. A full BLAKE3 digest makes distinct state files in
    // one parent effectively disjoint while keeping the prefix stable across
    // interrupted runs.
    let digest = blake3::hash(state_name.as_bytes()).to_hex();
    OsString::from(format!(".devenv-{kind}-{digest}-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn concurrent_first_opens_share_one_lock_file() {
        for _ in 0..20 {
            let temp = tempfile::tempdir().unwrap();
            let barrier = std::sync::Barrier::new(8);
            std::thread::scope(|scope| {
                let workers: Vec<_> = (0..8)
                    .map(|_| {
                        let path = temp.path().join("files.json");
                        let barrier = &barrier;
                        scope.spawn(move || {
                            let state = StateDirectory::new(&path).unwrap();
                            barrier.wait();
                            let file = state.open_lock().unwrap();
                            fs::fstat(&file).unwrap().st_ino
                        })
                    })
                    .collect();
                let inodes: Vec<_> = workers
                    .into_iter()
                    .map(|worker| worker.join().unwrap())
                    .collect();
                assert!(inodes.iter().all(|inode| *inode == inodes[0]));
            });
        }
    }

    #[test]
    fn rejects_special_files_and_symlinks_without_blocking() {
        for name in ["files.json", "files.lock", "files.recovery"] {
            for kind in ["fifo", "directory", "symlink"] {
                let temp = tempfile::tempdir().unwrap();
                let state = StateDirectory::new(&temp.path().join("files.json")).unwrap();
                let path = temp.path().join(name);
                match kind {
                    "fifo" => nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap(),
                    "directory" => std::fs::create_dir(&path).unwrap(),
                    _ => symlink("absent", &path).unwrap(),
                }
                let result = match name {
                    "files.json" => state.open_state().map(|_| ()),
                    "files.lock" => state.open_lock().map(|_| ()),
                    _ => state.open_recovery().map(|_| ()),
                };
                assert!(result.is_err(), "{name}: {kind}");
                if name == "files.recovery" {
                    assert!(state.append_recovery().is_err());
                }
            }
        }
    }

    #[test]
    fn state_and_lock_remain_on_the_anchored_parent() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("state");
        let moved = temp.path().join("moved-state");
        std::fs::create_dir(&parent).unwrap();
        let state = StateDirectory::new(&parent.join("files.json")).unwrap();

        std::fs::rename(&parent, &moved).unwrap();
        std::fs::create_dir(&parent).unwrap();
        drop(state.open_lock().unwrap());
        let (mut file, name) = state.temporary().unwrap();
        file.write_all(b"{}\n").unwrap();
        state.replace(&name).unwrap();

        assert!(moved.join("files.lock").is_file());
        assert_eq!(std::fs::read(moved.join("files.json")).unwrap(), b"{}\n");
        assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    }

    #[test]
    fn sweeps_only_private_regular_orphan_temporaries_on_the_anchored_parent() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("state");
        let moved = temp.path().join("moved-state");
        std::fs::create_dir(&parent).unwrap();
        let state = StateDirectory::new(&parent.join("files.json")).unwrap();
        let state_orphan = suffixed_name(&state.state_temporary_prefix, "orphan");
        let recovery_orphan = suffixed_name(&state.recovery_temporary_prefix, "orphan");
        for name in [&state_orphan, &recovery_orphan] {
            let path = parent.join(name);
            std::fs::write(&path, "orphan").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let public_name = suffixed_name(&state.state_temporary_prefix, "public");
        let public = parent.join(&public_name);
        std::fs::write(&public, "leave me").unwrap();
        let link_name = suffixed_name(&state.recovery_temporary_prefix, "link");
        symlink("nowhere", parent.join(&link_name)).unwrap();
        let legacy = parent.join(".devenv-state-legacy-orphan");
        std::fs::write(&legacy, "legacy").unwrap();
        std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o600)).unwrap();

        std::fs::rename(&parent, &moved).unwrap();
        std::fs::create_dir(&parent).unwrap();
        let impostor = parent.join(&state_orphan);
        std::fs::write(&impostor, "impostor").unwrap();
        std::fs::set_permissions(&impostor, std::fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(state.sweep_orphaned_temporaries().unwrap(), 2);
        assert!(!moved.join(&state_orphan).exists());
        assert!(!moved.join(&recovery_orphan).exists());
        assert!(moved.join(&public_name).is_file());
        assert!(std::fs::symlink_metadata(moved.join(&link_name)).is_ok());
        assert!(moved.join(".devenv-state-legacy-orphan").is_file());
        assert_eq!(std::fs::read(&impostor).unwrap(), b"impostor");
    }

    #[test]
    fn sweep_is_scoped_to_its_state_file_when_state_files_share_a_parent() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("state");
        std::fs::create_dir(&parent).unwrap();
        let first = StateDirectory::new(&parent.join("first.json")).unwrap();
        let second = StateDirectory::new(&parent.join("second.json")).unwrap();

        let (_, first_state_temporary) = first.temporary().unwrap();
        let (_, first_recovery_temporary) = first.recovery_temporary().unwrap();
        let (_, second_state_temporary) = second.temporary().unwrap();
        let (_, second_recovery_temporary) = second.recovery_temporary().unwrap();

        assert_ne!(first.state_temporary_prefix, second.state_temporary_prefix);
        assert_ne!(
            first.recovery_temporary_prefix,
            second.recovery_temporary_prefix
        );
        assert_eq!(first.sweep_orphaned_temporaries().unwrap(), 2);
        assert!(!parent.join(&first_state_temporary).exists());
        assert!(!parent.join(&first_recovery_temporary).exists());
        assert!(parent.join(&second_state_temporary).is_file());
        assert!(parent.join(&second_recovery_temporary).is_file());
        assert_eq!(second.sweep_orphaned_temporaries().unwrap(), 2);
    }

    fn suffixed_name(prefix: &OsStr, suffix: &str) -> OsString {
        let mut name = prefix.to_os_string();
        name.push(suffix);
        name
    }
}
