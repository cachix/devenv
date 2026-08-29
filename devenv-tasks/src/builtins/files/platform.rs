//! Native regular-file copying with portable fallbacks.
//!
//! rustix anchors creation to a directory descriptor. std::io::copy handles
//! data transfer and kernel fallbacks; extent queries preserve sparse holes.

use rustix::{
    fd::OwnedFd,
    fs::{Mode, OFlags},
};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    path::Path,
};
use tokio_util::sync::CancellationToken;

const COPY_CHUNK: u64 = 8 * 1024 * 1024;

/// Atomically creates `name` below `parent` and copies one regular file into it.
/// The name must be absent. Cancellation is observed between native and
/// buffered chunks, and a partial destination is removed on failure.
pub(super) fn copy_regular_file_at(
    source: &Path,
    parent: &OwnedFd,
    name: &OsStr,
    cancellation: &CancellationToken,
) -> io::Result<File> {
    check_cancelled(cancellation)?;

    // Sources follow symlinks, but a raced FIFO must not block the worker.
    let input = File::from(rustix::fs::open(
        source,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    if !input.metadata()?.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "source is not a regular file",
        ));
    }
    #[cfg(target_os = "macos")]
    match clone_file(&input, parent, name, cancellation) {
        Ok(()) => {
            let output = match rustix::fs::openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            ) {
                Ok(output) => output,
                Err(error) => {
                    let _ = rustix::fs::unlinkat(parent, name, rustix::fs::AtFlags::empty());
                    return Err(error.into());
                }
            };
            return Ok(File::from(output));
        }
        Err(error) if may_fallback(&error) => {}
        Err(error) => return Err(error),
    }

    let output = rustix::fs::openat(
        parent,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_bits_retain(0o600),
    )
    .map_err(io::Error::from)?;
    let result = copy_opened_file(input, File::from(output), cancellation);
    if result.is_err() {
        let _ = rustix::fs::unlinkat(parent, name, rustix::fs::AtFlags::empty());
    }
    result
}

fn copy_opened_file(
    mut input: File,
    mut output: File,
    cancellation: &CancellationToken,
) -> io::Result<File> {
    match copy_sparse(&mut input, &mut output, cancellation) {
        Ok(()) => Ok(output),
        Err(error) if may_fallback(&error) => {
            reset(&mut input, &mut output)?;
            copy_buffered(&mut input, &mut output, cancellation)?;
            Ok(output)
        }
        Err(error) => Err(error),
    }
}

fn check_cancelled(cancellation: &CancellationToken) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            ErrorKind::Interrupted,
            "file copy cancelled",
        ))
    } else {
        Ok(())
    }
}

fn reset(input: &mut File, output: &mut File) -> io::Result<()> {
    input.seek(SeekFrom::Start(0))?;
    output.set_len(0)?;
    output.seek(SeekFrom::Start(0))?;
    Ok(())
}

/// Errors which indicate that an optional kernel/filesystem operation is not
/// available for this pair of files, rather than that reading or writing failed.
#[cfg(unix)]
fn may_fallback(error: &io::Error) -> bool {
    error.kind() == ErrorKind::Unsupported
        || error.raw_os_error().is_some_and(|code| {
            [
                libc::EXDEV,
                libc::EINVAL,
                libc::ENOSYS,
                libc::EOPNOTSUPP,
                libc::ENOTSUP,
                libc::EPERM,
                libc::EACCES,
                libc::ENOTTY,
            ]
            .contains(&code)
        })
}

#[cfg(target_os = "macos")]
fn clone_file(
    input: &File,
    parent: &OwnedFd,
    name: &OsStr,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    check_cancelled(cancellation)?;
    // benchmark_copy_strategies on APFS: 64 MiB takes ~0.19 ms with cloning
    // versus ~40 ms with io::copy. fs::copy also clones, but accepts paths;
    // fclonefileat keeps creation anchored to the resolved staging directory.
    // `NOFOLLOW` also protects the destination on Darwin, so a raced dangling
    // symlink cannot redirect the staged clone. Do not inherit source ownership:
    // materializations are owned like freshly-created destination files.
    let flags = rustix::fs::CloneFlags::NOFOLLOW | rustix::fs::CloneFlags::NOOWNERCOPY;
    rustix::fs::fclonefileat(input, parent, name, flags).map_err(io::Error::from)?;
    if let Err(error) = check_cancelled(cancellation) {
        let _ = rustix::fs::unlinkat(parent, name, rustix::fs::AtFlags::empty());
        return Err(error);
    }
    Ok(())
}

/// Copies only data extents discovered through `SEEK_DATA`/`SEEK_HOLE`.
///
/// This keeps holes as holes on filesystems that expose their extent map. An
/// unsupported extent query returns a fallback-eligible error instead.
/// benchmark_copy_strategies: a 1 GiB sparse APFS file takes ~73 µs using
/// extents versus ~537 ms and 1 GiB of writes using io::copy alone.
#[cfg(unix)]
fn copy_sparse(
    input: &mut File,
    output: &mut File,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let length = input.metadata()?.len();
    output.set_len(length)?;
    let mut offset = 0_u64;
    while offset < length {
        check_cancelled(cancellation)?;
        let data = match rustix::fs::seek(&*input, rustix::fs::SeekFrom::Data(offset)) {
            Ok(data) => data,
            Err(rustix::io::Errno::NXIO) => break, // The remainder is a hole.
            Err(error) => return Err(error.into()),
        };
        let hole = rustix::fs::seek(&*input, rustix::fs::SeekFrom::Hole(data))
            .map_err(io::Error::from)?
            .min(length);
        if data < offset || hole <= data {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "filesystem returned an invalid sparse extent",
            ));
        }
        input.seek(SeekFrom::Start(data))?;
        output.seek(SeekFrom::Start(data))?;
        copy_exactly(input, output, hole - data, cancellation)?;
        offset = hole;
    }
    check_cancelled(cancellation)
}

fn copy_buffered(
    input: &mut File,
    output: &mut File,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let length = input.metadata()?.len();
    copy_exactly(input, output, length, cancellation)
}

fn copy_exactly(
    input: &mut File,
    output: &mut File,
    mut remaining: u64,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    while remaining != 0 {
        check_cancelled(cancellation)?;
        let wanted = remaining.min(COPY_CHUNK);
        // std handles short reads, EINTR and Linux copy_file_range fallback.
        let copied = io::copy(&mut (&mut *input).take(wanted), output)?;
        if copied != wanted {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "source changed while it was being copied",
            ));
        }
        remaining -= copied;
    }
    check_cancelled(cancellation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    #[test]
    fn rejects_fifo_sources_and_existing_destinations() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let parent = rustix::fs::open(
            temp.path(),
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .unwrap();
        let token = CancellationToken::new();
        nix::unistd::mkfifo(&source, nix::sys::stat::Mode::S_IRUSR).unwrap();
        assert_eq!(
            copy_regular_file_at(&source, &parent, OsStr::new("target"), &token)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
        assert!(!temp.path().join("target").exists());
        fs::remove_file(&source).unwrap();
        fs::write(&source, "new").unwrap();
        std::os::unix::fs::symlink("source", temp.path().join("target")).unwrap();
        assert!(copy_regular_file_at(&source, &parent, OsStr::new("target"), &token).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"new");
        assert_eq!(
            fs::read_link(temp.path().join("target")).unwrap(),
            Path::new("source")
        );
    }

    #[test]
    fn sparse_and_buffered_paths_cover_empty_holes_and_chunk_boundaries() {
        use std::os::unix::fs::MetadataExt;
        let temp = tempfile::tempdir().unwrap();
        for length in [0, 1, COPY_CHUNK - 1, COPY_CHUNK, COPY_CHUNK + 1] {
            for all_holes in [false, true] {
                let source = temp.path().join("source");
                let target = temp.path().join("target");
                let mut input = File::create(&source).unwrap();
                input.set_len(length).unwrap();
                if length > 0 && !all_holes {
                    input.write_all(b"a").unwrap();
                    input.seek(SeekFrom::End(-1)).unwrap();
                    input.write_all(b"z").unwrap();
                }
                drop(input);
                let mut input = File::open(&source).unwrap();
                let mut output = File::create(&target).unwrap();
                // Bypass cloning so the fallback is exercised on APFS too.
                output = copy_opened_file(input, output, &CancellationToken::new()).unwrap();
                input = File::open(&source).unwrap();
                assert_eq!(fs::read(&source).unwrap(), fs::read(&target).unwrap());
                let source_meta = input.metadata().unwrap();
                if source_meta.blocks() * 512 < length / 4 {
                    assert!(output.metadata().unwrap().blocks() * 512 < length / 2);
                }
                // A failed native operation may already have written data.
                output.write_all(b"partial native copy").unwrap();
                reset(&mut input, &mut output).unwrap();
                copy_buffered(&mut input, &mut output, &CancellationToken::new()).unwrap();
                assert_eq!(fs::read(&source).unwrap(), fs::read(&target).unwrap());
            }
        }
    }

    #[test]
    fn exact_copy_reports_truncation_and_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::write(&source, "short").unwrap();
        let mut input = File::open(source).unwrap();
        let mut output = tempfile::tempfile().unwrap();
        let token = CancellationToken::new();
        assert_eq!(
            copy_exactly(&mut input, &mut output, 100, &token)
                .unwrap_err()
                .kind(),
            ErrorKind::UnexpectedEof
        );
        token.cancel();
        assert_eq!(
            copy_exactly(&mut input, &mut output, 0, &token)
                .unwrap_err()
                .kind(),
            ErrorKind::Interrupted
        );
    }

    #[test]
    // Compare against std with --ignored --nocapture; no CI timing thresholds.
    #[ignore = "manual copy strategy benchmark"]
    fn benchmark_copy_strategies() {
        use std::time::Instant;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let parent = rustix::fs::open(
            temp.path(),
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .unwrap();
        for (label, bytes, sparse) in [
            ("small", 64 * 1024, false),
            ("large", 64 * 1024 * 1024, false),
            ("sparse", 1024 * 1024 * 1024, true),
        ] {
            let mut file = File::create(&source).unwrap();
            if sparse {
                file.set_len(bytes).unwrap();
                file.seek(SeekFrom::End(-1)).unwrap();
                file.write_all(b"x").unwrap();
            } else {
                file.write_all(&vec![b'x'; bytes as usize]).unwrap();
            }
            // Btrfs may flush dirty source extents before cloning them. Do
            // that once before timing, rather than charge the first strategy.
            file.sync_all().unwrap();
            drop(file);
            for strategy in ["native", "extents", "io-copy", "fs-copy"] {
                let mut elapsed = std::time::Duration::ZERO;
                for _ in 0..5 {
                    let target = temp.path().join("target");
                    let start = Instant::now();
                    match strategy {
                        "extents" => {
                            copy_opened_file(
                                File::open(&source).unwrap(),
                                File::create(&target).unwrap(),
                                &CancellationToken::new(),
                            )
                            .unwrap();
                        }
                        "native" => {
                            copy_regular_file_at(
                                &source,
                                &parent,
                                OsStr::new("target"),
                                &CancellationToken::new(),
                            )
                            .unwrap();
                        }
                        "io-copy" => {
                            io::copy(
                                &mut File::open(&source).unwrap(),
                                &mut File::create(&target).unwrap(),
                            )
                            .unwrap();
                        }
                        _ => {
                            fs::copy(&source, &target).unwrap();
                        }
                    }
                    elapsed += start.elapsed();
                    assert_eq!(fs::metadata(&target).unwrap().len(), bytes);
                    fs::remove_file(target).unwrap();
                }
                eprintln!("{label} {strategy}: {:?} mean", elapsed / 5);
            }
        }
    }

    #[test]
    fn copies_regular_file_contents() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"a file with some contents").unwrap();
        let parent = rustix::fs::open(
            directory.path(),
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .unwrap();

        copy_regular_file_at(
            &source,
            &parent,
            destination.file_name().unwrap(),
            &CancellationToken::new(),
        )
        .unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"a file with some contents");
    }

    #[test]
    fn cancellation_before_copy_preserves_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"new contents").unwrap();
        fs::write(&destination, b"old contents").unwrap();
        let parent = rustix::fs::open(
            directory.path(),
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
        .unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = copy_regular_file_at(
            &source,
            &parent,
            destination.file_name().unwrap(),
            &cancellation,
        )
        .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Interrupted);
        assert_eq!(fs::read(destination).unwrap(), b"old contents");
    }
}
