//! Pre-bootstrap lock-file helpers.
//!
//! Free functions over an explicit `EvalState` + `Store` + settings.
//! Lock helpers never open a store; the caller controls eval-state lifecycle.
//! Wrap construction + validation in [`with_lock_scope`] so the lazy
//! `«nix-internal»/derivation-internal.nix` load nests under the activity.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use devenv_activity::{Activity, ActivityLevel, instrument_activity};
use devenv_core::config::Input;
use devenv_core::nix_log_bridge::NixLogBridge;
use miette::{Result, WrapErr};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;

/// Build a transient `EvalState` for lock-file work. Drop it when done.
pub fn build_eval_state(
    store: &Store,
    root: &Path,
    flake_settings: &FlakeSettings,
    refresh_fetchers: bool,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?;
    let builder = EvalStateBuilder::new(store.clone())
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .base_directory(root_str)
        .to_miette()
        .wrap_err("Failed to set base directory")?
        .flakes(flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?;
    apply_fetcher_refresh(builder, refresh_fetchers)?
        .build()
        .to_miette()
        .wrap_err("Failed to build eval state")
}

/// Apply `--refresh` semantics to an eval state's fetcher cache.
pub(crate) fn apply_fetcher_refresh(
    mut builder: EvalStateBuilder,
    refresh_fetchers: bool,
) -> Result<EvalStateBuilder> {
    if refresh_fetchers {
        builder = builder
            .fetch_setting("tarball-ttl", "0")
            .to_miette()
            .wrap_err("Failed to set tarball-ttl")?;
    }
    Ok(builder)
}

/// Run `f` inside a "Validating lock" activity + `begin_eval` scope.
///
/// Wrap any `EvalState` construction and validation here so Nix's lazy
/// `«nix-internal»` loads nest under the activity.
pub fn with_lock_scope<F, T>(bridge: &Arc<NixLogBridge>, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let activity =
        devenv_activity::start!(Activity::evaluate("Validating lock").level(ActivityLevel::Info));
    let _eval_guard = bridge.begin_eval_with_span(activity.id(), activity.span());
    activity.with_new_scope_sync(f)
}

/// Validate (and create or update if needed) `lock_file`,
/// returning the fingerprint of the resulting lock graph.
pub fn validate_and_load(
    eval_state: &EvalState,
    store: &Store,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    lock_file: &Path,
    inputs: &BTreeMap<String, Input>,
) -> Result<String> {
    crate::validate_lock_file(
        eval_state,
        fetchers_settings,
        flake_settings,
        root,
        lock_file,
        inputs,
    )
    .to_miette()?;
    fingerprint(store, fetchers_settings, lock_file)
}

/// Compute the fingerprint of `lock_file` against `store`.
pub fn fingerprint(
    store: &Store,
    fetchers_settings: &FetchersSettings,
    lock_file: &Path,
) -> Result<String> {
    let lock = crate::load_lock_file(fetchers_settings, lock_file).to_miette()?;
    crate::compute_lock_fingerprint(lock.as_ref(), fetchers_settings, store).to_miette()
}

/// Lock or update the requested inputs.
#[instrument_activity("Updating lock", kind = evaluate, level = DEBUG)]
pub fn update(
    eval_state: &EvalState,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    lock_file: &Path,
    inputs: &BTreeMap<String, Input>,
    name: Option<&str>,
    overrides: &[String],
) -> Result<()> {
    crate::lock_inputs(
        eval_state,
        fetchers_settings,
        flake_settings,
        root,
        lock_file,
        inputs,
        name,
        overrides,
    )
    .to_miette()
}

#[cfg(all(test, feature = "test-nix-store"))]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use devenv_core::{NixSettings, StoreSettings};

    use super::*;

    fn write_octal(field: &mut [u8], value: u64) {
        let value = format!("{value:o}");
        field.fill(b'0');
        let start = field.len() - value.len() - 1;
        field[start..start + value.len()].copy_from_slice(value.as_bytes());
        field[field.len() - 1] = 0;
    }

    fn tarball() -> Vec<u8> {
        let contents = b"cached";
        let mut header = [0_u8; 512];
        header[..12].copy_from_slice(b"source/value");
        write_octal(&mut header[100..108], 0o644);
        write_octal(&mut header[108..116], 0);
        write_octal(&mut header[116..124], 0);
        write_octal(&mut header[124..136], contents.len() as u64);
        write_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());

        let mut archive = Vec::from(header);
        archive.extend_from_slice(contents);
        archive.resize(1024, 0);
        archive.resize(2048, 0);
        archive
    }

    fn fetch_tarball(eval_state: &mut EvalState, root: &Path, url: &str) {
        let url = ser_nix::to_string(&url).unwrap();
        let expression = format!("toString (builtins.fetchTarball {url})");
        let value = eval_state
            .eval_from_string(&expression, root.to_str().unwrap())
            .unwrap();
        eval_state.realise_string(&value, false).unwrap();
    }

    #[test]
    fn fetcher_refresh_does_not_leak_between_evaluators() {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping: binding a TCP socket is not permitted: {error}");
                return;
            }
            Err(error) => panic!("failed to bind a TCP listener: {error}"),
        };
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let stopped = Arc::new(AtomicBool::new(false));
        let server_stopped = Arc::clone(&stopped);
        let archive = tarball();
        let server = std::thread::spawn(move || {
            while !server_stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if server_stopped.load(Ordering::Relaxed) {
                            break;
                        }
                        let mut request = [0_u8; 4096];
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let _ = stream.read(&mut request);
                        server_requests.fetch_add(1, Ordering::Relaxed);
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/x-tar\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            archive.len()
                        );
                        let _ = stream.write_all(&archive);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("HTTP server failed: {error}"),
                }
            }
        });

        let root = tempfile::tempdir().unwrap();
        let nix_settings = NixSettings::default();
        let store_settings = StoreSettings::default();
        let _gc_registration = crate::backend::init_nix(&nix_settings, &store_settings).unwrap();
        let store = crate::backend::open_store(&store_settings).unwrap();
        let (flake_settings, _) = crate::backend::build_settings().unwrap();
        let url = format!("http://{address}/{}/source.tar", uuid::Uuid::new_v4());

        let mut refreshing = build_eval_state(&store, root.path(), &flake_settings, true).unwrap();
        fetch_tarball(&mut refreshing, root.path(), &url);
        drop(refreshing);
        let first_requests = requests.load(Ordering::Relaxed);

        let mut cached = build_eval_state(&store, root.path(), &flake_settings, false).unwrap();
        fetch_tarball(&mut cached, root.path(), &url);
        let cached_requests = requests.load(Ordering::Relaxed);

        stopped.store(true, Ordering::Relaxed);
        TcpStream::connect(address).unwrap();
        server.join().unwrap();

        assert!(first_requests > 0);
        assert_eq!(cached_requests, first_requests);
    }
}
