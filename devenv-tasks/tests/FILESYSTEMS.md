The files runner uses `rustix` for descriptor-relative destination operations,
`ignore` for source traversal, `tempfile` for state temporaries, `fd-lock` for
serialization, and `std::io::copy` for data transfer. Source traversal disables
ignore filters and follows symlinks; destination operations must remain anchored
when parents move or are replaced by symlinks.

Run the ordinary suite with:

```sh
devenv shell -- cargo nextest run -p devenv-tasks --features test-all
```

Run the volume matrix on a disposable machine with:

```sh
devenv shell -- bash devenv-tasks/tests/filesystems.sh
```

The script mounts fresh volumes, runs the file unit, integration, and SIGKILL
recovery tests on each, and removes the volumes afterward. Linux requires sudo,
loop mounts, and the ext4/Btrfs/XFS formatting tools. macOS uses `hdiutil`.
`.github/workflows/filesystems.yml` runs this on hosted Linux and macOS runners;
the regular PR pipeline also tests Linux on x86_64 and aarch64 and macOS on aarch64.

| Filesystem | Coverage |
| --- | --- |
| ext4 | Linux ordinary copying and sparse extents |
| Btrfs, XFS | Linux filesystems with native copy acceleration |
| tmpfs | Memory-backed files and kernel-copy fallbacks |
| APFS | macOS cloning, sparse extents, case-insensitive names |
| Case-sensitive APFS | Distinct case variants on macOS |
| HFS+ | Clone/extent fallback and whole-second timestamps |

Every matrix entry requires a source on a different device, using
`DEVENV_FILES_SOURCE_DIR`. Ordinary local runs use `/dev/shm` when available.
Tests also bypass cloning to exercise extent/buffered copying on APFS. Sparse
allocation is asserted when the source filesystem supports holes. Content hashes
protect against rapid same-length edits: HFS+ timestamps have whole-second
resolution, and Linux timestamps can collide within a kernel tick. Immutable
Nix-store sources retain their shortcut. Mutable sources and destination files
must be read during validation; watching cannot replace that check.

Performance probes are opt-in and have no CI timing thresholds:

```sh
devenv shell -- cargo test -p devenv-tasks --lib benchmark_ -- --ignored --nocapture
devenv shell -- cargo test -p devenv-tasks --test files_builtin_benchmarks -- --ignored --nocapture
```

On macOS/APFS, a debug build measured these costs on 2026-09-05. Copy timings
average five copies; fingerprint timings average ten checks. They justify the
remaining fast paths, rather than serving as portable performance expectations.

| Operation | Retained implementation | Comparison |
| --- | --- | --- |
| Copy 64 MiB | `fclonefileat`: 0.19 ms | `io::copy`: 34 ms |
| Copy 1 GiB sparse file | Extent copying: 0.073 ms | `io::copy`: 537 ms |
| Fingerprint 4,096 copied entries | Reuse staged digest: 0.012 ms | Walk again: 69 ms |
| Resolve 4,096 paths, four parents each | Cached parents: 3 ms total | Reopen parents: 147 ms total |

`std::fs::copy` also clones efficiently on APFS, but its pathname API cannot
preserve the destination descriptor guarantee. Linux data transfers delegate
kernel acceleration and its fallback handling to `std::io::copy`.

The copy timings above measure transfer only; content validation adds reads.
The [`std::io::copy` Linux implementation](https://doc.rust-lang.org/src/std/sys/io/kernel_copy/linux.rs.html)
recognizes `Take<&mut File>`, so cancellation chunking retains kernel offload.
The [kernel timestamp documentation](https://docs.kernel.org/filesystems/multigrain-ts.html)
explains why nanosecond fields alone are insufficient for detecting writes.

[Watchexec](https://docs.rs/watchexec/latest/watchexec/sources/fs/index.html)
and [notify](https://docs.rs/notify/8.2.0/notify/) expose OS-specific watching and
content-comparing polling, but no reusable copy runner or persistent ownership
fingerprints. Watch notifications can be missing or coalesced.
`cap-std` and `reflink-copy` offer native copying, but their copy APIs do not
combine exclusive, no-follow staging with this runner's cancellation contract.

Content validation has a measurable cost. In the same unoptimized lifecycle
probe, 256 flat 64 KiB copies took 165 ms on an unchanged run versus 9 ms with
metadata-only validation; a 256-file tree took 33 ms versus 4 ms. These are debug
build measurements, not release throughput figures. This trades the old
metadata-only shortcut for reliable detection of same-size edits, and is
consistent with `devenv-cache-core`'s content-based file tracking.

Validated on x86_64 Linux 7.1.8 with Rust 1.97.1 on 2026-09-05:
all 270 task-runner tests passed, and all 81 filesystem tests passed on each of
ext4, Btrfs, XFS, and tmpfs. The opt-in cancellation test and all three filesystem
microbenchmarks also passed. The temporary checkout, Cargo/build caches, test
mounts, and checkout-specific GC roots were removed afterward.

Copy benchmarks synchronize the source before timing: Btrfs can flush dirty
source extents before cloning, which would otherwise charge only the first
strategy for fixture preparation.

The mainline comparisons below were measured on 2026-09-05 on macOS 27.0,
aarch64, APFS, and Linux 7.1.8, x86_64, Btrfs (Rust 1.97.1), using
`cargo build --locked --release -p devenv-tasks` on each platform.
The baseline uses the actual `src/modules/files.nix` cleanup/create scripts from
mainline commit `360b5eb1397291383d10845a63a0247981bd5598`, evaluated with the
same pinned nixpkgs as this checkout and wrapped in Bash with `set -e`, as in
the task module. Both variants use the same optimized task-runner binary to
isolate the change from shell tasks to the native builtin.

Each fixture has 256 flat destinations referencing one immutable 64 KiB
Nix-store file. Each sample starts with an empty project, state, task cache,
and runtime directory, then runs creation followed immediately by an unchanged
run. These are medians of five samples, alternating which runner executes
first. Timings include process startup, task scheduling, state/cache access,
and both mainline tasks; they exclude Nix evaluation and script generation.
Runs use `devenv-tasks run devenv:files --mode all --on-idle exit` with separate
task files, caches, and runtime directories. Both runners' outputs were checked;
the native runner also preserved writable-copy inodes on unchanged runs.

| Platform | Workload | Mainline shell tasks | Native builtin | Speedup |
| --- | --- | --- | --- | --- |
| macOS/APFS | Create 256 symlinks | 1,553.6 ms | 27.9 ms | 55.6× |
| macOS/APFS | Check 256 unchanged symlinks | 1,612.2 ms | 9.5 ms | 169.4× |
| macOS/APFS | Create 256 writable 64 KiB copies | 2,429.0 ms | 243.9 ms | 10.0× |
| macOS/APFS | Reconcile 256 unchanged writable 64 KiB copies | 3,965.8 ms | 23.5 ms | 168.8× |
| Linux/Btrfs | Create 256 symlinks | 346.9 ms | 32.9 ms | 10.5× |
| Linux/Btrfs | Check 256 unchanged symlinks | 534.8 ms | 7.6 ms | 70.0× |
| Linux/Btrfs | Create 256 writable 64 KiB copies | 509.2 ms | 223.6 ms | 2.3× |
| Linux/Btrfs | Reconcile 256 unchanged writable 64 KiB copies | 972.1 ms | 12.9 ms | 75.6× |

Mainline recreates writable copies on every run; the builtin validates their
contents and preserves unchanged files. These measurements cover file task
execution for this fixture on each platform. The temporary Linux checkout,
Cargo/build caches, fixtures, and checkout-specific GC roots were removed
afterward.
