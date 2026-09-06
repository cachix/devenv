//! Automatic integration with the shared `devenv-proxy` daemon.

use crate::tasks;
use devenv_mailbox::FrontendCommand;
use devenv_proxy::{ControlRequest, ControlResponse, Route, TlsConfig};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const START_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_PROJECT_NAME: &str = "devenv-shell";

pub(crate) async fn prepare_https(
    routes: &mut [Route],
    task_configs: &mut [tasks::TaskConfig],
    envs: &HashMap<String, String>,
    frontend: Option<&tokio::sync::mpsc::Sender<FrontendCommand>>,
) -> Result<()> {
    let tls_routes = https_routes(routes, task_configs);
    if tls_routes.is_empty() {
        return Ok(());
    }
    let ca_dir = envs
        .get("CAROOT")
        .map(PathBuf::from)
        .ok_or_else(|| miette!("HTTPS requires the project's mkcert CAROOT"))?;
    let mkcert = envs
        .get("DEVENV_MKCERT")
        .ok_or_else(|| miette!("HTTPS requires the Nix-provided mkcert executable"))?;
    let command = || {
        let mut command = Command::new(mkcert);
        command.envs(envs).env("CAROOT", &ca_dir);
        command
    };
    fs::create_dir_all(&ca_dir).into_diagnostic()?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(ca_dir.join("proxy.lock"))
        .into_diagnostic()?;
    lock.lock()
        .into_diagnostic()
        .wrap_err("failed to lock proxy certificate setup")?;
    if !ca_dir.join("rootCA.pem").exists() {
        let mut install = command();
        install.arg("-install");
        let status = run_certificate_install(&mut install, frontend).await?;
        if !status.success() {
            devenv_activity::message(
                devenv_activity::ActivityLevel::Warn,
                "mkcert could not install the local CA into trust stores; run `mkcert -install` in the development shell to trust HTTPS URLs",
            );
        }
        if !ca_dir.join("rootCA.pem").exists() {
            bail!(
                "mkcert failed to create the local certificate authority in {}",
                ca_dir.display()
            );
        }
    }

    let directory = ca_dir.join("proxy");
    fs::create_dir_all(&directory).into_diagnostic()?;
    let tls = TlsConfig {
        certificate: directory.join("cert.pem"),
        key: directory.join("key.pem"),
    };
    let mut hostnames: Vec<_> = tls_routes
        .iter()
        .map(|route| route.hostname.clone())
        .collect();
    hostnames.sort();
    hostnames.dedup();
    if !tls.is_current(&hostnames, &ca_dir.join("rootCA.pem")) {
        devenv_activity::message(
            devenv_activity::ActivityLevel::Info,
            "generating certificates for HTTPS process URLs",
        );
        // Generate a complete pair before replacing the files used by the proxy.
        let temporary = tempfile::tempdir_in(&directory).into_diagnostic()?;
        let certificate = temporary.path().join("cert.pem");
        let key = temporary.path().join("key.pem");
        let output = command()
            .arg("-cert-file")
            .arg(&certificate)
            .arg("-key-file")
            .arg(&key)
            .args(&hostnames)
            .stdin(Stdio::null())
            .output()
            .into_diagnostic()
            .wrap_err("failed to run mkcert for proxy hostnames")?;
        if !output.status.success() {
            bail!(
                "mkcert failed to generate proxy certificates: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        fs::rename(certificate, &tls.certificate).into_diagnostic()?;
        fs::rename(key, &tls.key).into_diagnostic()?;
    }
    for route in tls_routes {
        route.tls = Some(tls.clone());
    }
    update_proxy_urls(routes, task_configs);
    Ok(())
}

fn update_proxy_urls(routes: &[Route], task_configs: &mut [tasks::TaskConfig]) {
    // Keep displayed URLs derived from the actual registered routes.
    let urls: HashMap<_, _> = routes
        .iter()
        .map(|route| {
            let mut http = route.clone();
            http.tls = None;
            (route_url(&http), route_url(route))
        })
        .collect();
    for task in task_configs {
        if let Some(process) = &mut task.process {
            for url in &mut process.proxy.urls {
                if let Some(https) = urls.get(url) {
                    *url = https.clone();
                }
            }
        }
    }
}

fn https_routes<'a>(
    routes: &'a mut [Route],
    task_configs: &[tasks::TaskConfig],
) -> Vec<&'a mut Route> {
    let urls: HashSet<_> = task_configs
        .iter()
        .filter_map(|task| task.process.as_ref())
        .filter(|process| process.proxy.https.enable)
        .flat_map(|process| process.proxy.urls.iter().map(String::as_str))
        .collect();
    routes
        .iter_mut()
        .filter(|route| urls.contains(route_url(route).as_str()))
        .collect()
}

async fn run_certificate_install(
    command: &mut Command,
    frontend: Option<&tokio::sync::mpsc::Sender<FrontendCommand>>,
) -> Result<std::process::ExitStatus> {
    let mut resume = None;
    if let Some(frontend) = frontend {
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
        frontend
            .send(FrontendCommand::PauseForInteraction {
                ready: ready_tx,
                resume: resume_rx,
            })
            .await
            .map_err(|_| {
                miette!("terminal frontend stopped before certificate trust installation")
            })?;
        tokio::task::spawn_blocking(move || ready_rx.recv())
            .await
            .into_diagnostic()?
            .map_err(|_| {
                miette!("terminal frontend stopped before certificate trust installation")
            })?;
        resume = Some(resume_tx);
    }
    let result = command
        .status()
        .into_diagnostic()
        .wrap_err("failed to install the local certificate authority");
    if let Some(resume) = resume {
        let _ = resume.send(());
    }
    result
}

pub(crate) fn project_name(configured: Option<String>, root: &Path) -> Result<String> {
    configured
        .filter(|name| !name.trim().is_empty() && name != DEFAULT_PROJECT_NAME)
        .or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .ok_or_else(|| miette!("could not derive a project name for localhost proxy routes"))
}

pub(crate) fn project_routes(
    project_name: &str,
    owner: &str,
    task_configs: &mut [tasks::TaskConfig],
) -> Result<Vec<Route>> {
    let project = hostname_label(project_name)?;
    let mut routes = Vec::new();
    let mut hostnames = HashSet::new();

    for task in task_configs {
        let Some(process_name) = task.name.strip_prefix(devenv_tasks::PROCESS_TASK_PREFIX) else {
            continue;
        };
        let Some(process) = task.process.as_mut() else {
            continue;
        };
        process.proxy.urls.clear();
        if process.ports.is_empty() {
            continue;
        }
        let first_route = routes.len();

        let base_hostname = match process.proxy.hostname.as_deref() {
            Some(hostname) => devenv_proxy::normalize_hostname(hostname).map_err(|error| {
                miette!("invalid proxy hostname for process {process_name}: {error}")
            })?,
            None => format!("{}.{}.localhost", hostname_label(process_name)?, project),
        };
        let ports: BTreeMap<&str, u16> = process
            .ports
            .iter()
            .map(|(name, port)| (name.as_str(), *port))
            .collect();
        let default_port = ports
            .get_key_value("http")
            .map(|(name, port)| (*name, *port))
            .or_else(|| {
                (ports.len() == 1).then(|| {
                    let (name, port) = ports.first_key_value().unwrap();
                    (*name, *port)
                })
            });

        if let Some((port_name, port)) = default_port {
            let hostname = port_hostname(process_name, process, port_name)?
                .unwrap_or_else(|| base_hostname.clone());
            push_route(&mut routes, &mut hostnames, hostname, port, owner)?;
        }

        // Multiple named ports remain addressable without requiring another
        // option. A port-specific hostname replaces its generated route; the
        // conventional `http` port otherwise also receives the short URL.
        if ports.len() > 1 {
            for (port_name, port) in ports {
                let configured_hostname = port_hostname(process_name, process, port_name)?;
                if default_port.is_some_and(|(default_name, _)| default_name == port_name)
                    && configured_hostname.is_some()
                {
                    continue;
                }
                let port_label = hostname_label(port_name)?;
                push_route(
                    &mut routes,
                    &mut hostnames,
                    configured_hostname.unwrap_or_else(|| format!("{port_label}.{base_hostname}")),
                    port,
                    owner,
                )?;
            }
        }
        process.proxy.urls = routes[first_route..].iter().map(route_url).collect();
    }

    Ok(routes)
}

fn route_url(route: &Route) -> String {
    let (scheme, default_port, listen) = if route.tls.is_some() {
        ("https", 443, proxy_https_listen_address())
    } else {
        ("http", 80, proxy_listen_address())
    };
    let port = listen.map(|address| address.port()).unwrap_or(default_port);
    if port == default_port {
        format!("{scheme}://{}", route.hostname)
    } else {
        format!("{scheme}://{}:{port}", route.hostname)
    }
}

fn port_hostname(
    process_name: &str,
    process: &devenv_processes::ProcessConfig,
    port_name: &str,
) -> Result<Option<String>> {
    process
        .proxy
        .port_hostnames
        .get(port_name)
        .map(|hostname| {
            devenv_proxy::normalize_hostname(hostname).map_err(|error| {
                miette!(
                    "invalid proxy hostname for port {port_name} of process {process_name}: {error}"
                )
            })
        })
        .transpose()
}

fn push_route(
    routes: &mut Vec<Route>,
    hostnames: &mut HashSet<String>,
    hostname: String,
    port: u16,
    owner: &str,
) -> Result<()> {
    if !hostnames.insert(hostname.clone()) {
        bail!("multiple process ports resolve to proxy hostname {hostname}");
    }
    routes.push(Route {
        hostname,
        upstream: SocketAddr::from(([127, 0, 0, 1], port)),
        owner: owner.to_owned(),
        tls: None,
    });
    Ok(())
}

fn hostname_label(value: &str) -> Result<String> {
    let mut label = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !label.is_empty() {
                label.push('-');
            }
            label.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if label.is_empty() {
        bail!("{value:?} cannot be represented as a localhost hostname label");
    }
    if label.len() > 63 {
        bail!("{value:?} is too long for a localhost hostname label");
    }
    Ok(label)
}

pub(crate) async fn reconcile(
    owner: &str,
    routes: Vec<Route>,
    frontend: Option<&tokio::sync::mpsc::Sender<FrontendCommand>>,
) -> Result<()> {
    if routes.is_empty() {
        // Do not start a machine-wide listener for a project with no declared
        // ports, but do remove routes left by an earlier configuration.
        let _ = replace_owner(owner, routes);
        return Ok(());
    }

    ensure_running(frontend, routes.iter().any(|route| route.tls.is_some())).await?;
    replace_owner(owner, routes.clone())?;
    for route in routes {
        devenv_activity::message(
            devenv_activity::ActivityLevel::Info,
            format!("{} -> http://{}", route_url(&route), route.upstream),
        );
    }
    Ok(())
}

pub(crate) fn clear(owner: &str) {
    let _ = replace_owner(owner, Vec::new());
}

fn replace_owner(owner: &str, routes: Vec<Route>) -> Result<()> {
    let socket = devenv_proxy::default_control_socket();
    devenv_proxy::request(
        &socket,
        &ControlRequest::ReplaceOwner {
            owner: owner.to_owned(),
            routes,
        },
    )
    .and_then(ControlResponse::into_result)
    .map(|_| ())
    .map_err(|error| miette!("{error:#}"))
    .wrap_err_with(|| format!("failed to update localhost proxy via {}", socket.display()))
}

async fn ensure_running(
    frontend: Option<&tokio::sync::mpsc::Sender<FrontendCommand>>,
    https: bool,
) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    let _ = frontend;

    let socket = devenv_proxy::default_control_socket();
    let listen = proxy_listen_address()
        .ok_or_else(|| miette!("DEVENV_PROXY_LISTEN is not a valid socket address"))?;
    let https_listen = https
        .then(|| {
            proxy_https_listen_address()
                .ok_or_else(|| miette!("DEVENV_PROXY_HTTPS_LISTEN is not a valid socket address"))
        })
        .transpose()?;
    if wait_for_existing_proxy(&socket, listen).await? {
        if let Some(expected) = https_listen {
            match devenv_proxy::request(&socket, &ControlRequest::Status) {
                Ok(ControlResponse::Info {
                    https_listen: Some(actual),
                    ..
                }) if actual == expected => {}
                _ => bail!(
                    "the running devenv-proxy has no HTTPS listener at {expected}; restart the shared proxy and run `devenv up` again"
                ),
            }
        }
        return Ok(());
    }

    let executable = proxy_executable()?;
    let log_path = proxy_log_path(&socket);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to open {}", log_path.display()))?;
    let mut args = vec![
        "--listen".to_owned(),
        listen.to_string(),
        "--control-socket".to_owned(),
        socket
            .to_str()
            .ok_or_else(|| miette!("proxy control socket path is not valid UTF-8"))?
            .to_owned(),
    ];
    if let Some(listen) = https_listen {
        args.extend(["--https-listen".to_owned(), listen.to_string()]);
    }

    #[cfg(target_os = "linux")]
    let detached_pid = if (listen.port() < 1024
        || https_listen.is_some_and(|listen| listen.port() < 1024))
        && !nix::unistd::geteuid().is_root()
    {
        let program = executable
            .to_str()
            .ok_or_else(|| miette!("proxy executable path is not valid UTF-8"))?;
        let runtime_dir = socket
            .parent()
            .ok_or_else(|| miette!("proxy control socket has no parent directory"))?;
        let cwd = std::env::current_dir()
            .into_diagnostic()
            .wrap_err("failed to determine the proxy working directory")?;
        Some(
            devenv_processes::start_capability_daemon(
                devenv_processes::CapabilityRequest::new(
                    "devenv-proxy",
                    vec!["net_bind_service".to_owned()],
                ),
                runtime_dir,
                frontend,
                program,
                &args,
                &std::env::vars().collect::<HashMap<_, _>>(),
                &cwd,
                &log_path,
            )
            .await
            .wrap_err("failed to start the proxy with Linux low-port access")?,
        )
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let detached_pid: Option<u32> = None;

    let mut child = if detached_pid.is_none() {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to open {}", log_path.display()))?;
        let stderr = log.try_clone().into_diagnostic()?;
        let mut command = Command::new(&executable);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Some(command.spawn().into_diagnostic().wrap_err_with(|| {
            format!(
                "failed to start internal proxy executable {}",
                executable.display()
            )
        })?)
    } else {
        None
    };
    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if proxy_ready(&socket) {
            return Ok(());
        }
        if let Some(status) = child
            .as_mut()
            .map(|child| child.try_wait())
            .transpose()
            .into_diagnostic()?
            .flatten()
        {
            let detail = fs::read_to_string(&log_path).unwrap_or_default();
            bail!(
                "devenv-proxy exited with {status}; it must be allowed to bind {listen}\n{}",
                detail.trim()
            );
        }
        #[cfg(target_os = "linux")]
        if detached_pid.is_some_and(|pid| !linux_process_running(pid)) {
            let detail = fs::read_to_string(&log_path).unwrap_or_default();
            bail!("devenv-proxy exited during startup\n{}", detail.trim());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    bail!(
        "devenv-proxy did not become ready within {}s; see {}",
        START_TIMEOUT.as_secs(),
        log_path.display()
    )
}

#[cfg(target_os = "linux")]
fn linux_process_running(pid: u32) -> bool {
    i32::try_from(pid).is_ok_and(|pid| {
        // The broker reports the PID before its child drops root privileges.
        // EPERM means the child exists but we cannot signal it yet.
        matches!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Ok(()) | Err(nix::errno::Errno::EPERM)
        )
    })
}

fn proxy_ready(socket: &Path) -> bool {
    proxy_control_ready(socket) && proxy_listen_address().is_some_and(proxy_data_plane_ready)
}

fn proxy_control_ready(socket: &Path) -> bool {
    devenv_proxy::request(socket, &ControlRequest::List)
        .and_then(ControlResponse::into_result)
        .is_ok()
}

async fn wait_for_existing_proxy(socket: &Path, listen: SocketAddr) -> Result<bool> {
    let started = Instant::now();
    // During startup and shutdown the control socket can outlive the HTTP
    // listener. Wait for that daemon to become ready or release its socket
    // before asking for capabilities and attempting to launch another one.
    while proxy_control_ready(socket) {
        if proxy_data_plane_ready(listen) {
            return Ok(true);
        }
        if started.elapsed() >= START_TIMEOUT {
            bail!(
                "the existing devenv-proxy still owns {} but its HTTP listener at {listen} is unavailable; see {}",
                socket.display(),
                proxy_log_path(socket).display(),
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(false)
}

fn proxy_data_plane_ready(address: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) else {
        return false;
    };
    if stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_millis(200)))
            .is_err()
    {
        return false;
    }
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        devenv_proxy::HEALTH_HOSTNAME
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = [0_u8; 32];
    stream
        .read(&mut response)
        .is_ok_and(|length| response[..length].starts_with(b"HTTP/1.1 204"))
}

fn proxy_listen_address() -> Option<SocketAddr> {
    std::env::var("DEVENV_PROXY_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:80".to_owned())
        .parse()
        .ok()
}

fn proxy_https_listen_address() -> Option<SocketAddr> {
    std::env::var("DEVENV_PROXY_HTTPS_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:443".to_owned())
        .parse()
        .ok()
}

fn proxy_executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEVENV_PROXY_BINARY") {
        return Ok(PathBuf::from(path));
    }
    bundled_proxy_executable()
}

fn bundled_proxy_executable() -> Result<PathBuf> {
    let current = std::env::current_exe()
        .into_diagnostic()
        .wrap_err("failed to locate the devenv executable")?;
    if let Some(sibling) = current.parent().map(|parent| parent.join("devenv-proxy"))
        && sibling.is_file()
    {
        return Ok(sibling);
    }
    which::which("devenv-proxy")
        .into_diagnostic()
        .wrap_err("devenv-proxy is missing from the devenv installation")
}

fn proxy_log_path(socket: &Path) -> PathBuf {
    socket
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("proxy.log")
}

#[cfg(test)]
mod tests {
    use super::*;
    use devenv_processes::{ProcessConfig, ProcessProxyConfig};

    #[cfg(target_os = "linux")]
    #[test]
    fn privileged_process_is_not_mistaken_for_an_exited_daemon() {
        // PID 1 exists, but an unprivileged caller cannot signal the root-owned
        // init process. This is also true of the broker child before setresuid.
        assert!(linux_process_running(1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exited_daemon_is_not_running() {
        let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!linux_process_running(pid));
        assert!(!linux_process_running(u32::MAX));
    }

    #[tokio::test]
    async fn certificate_trust_requires_frontend_handoff() {
        let (frontend, receiver) = tokio::sync::mpsc::channel(1);
        drop(receiver);
        let error = run_certificate_install(&mut Command::new("must-not-run"), Some(&frontend))
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("terminal frontend stopped before certificate trust installation")
        );
    }

    #[test]
    fn certificate_routes_use_https_urls() {
        let route = Route {
            hostname: "docs.demo.localhost".to_owned(),
            upstream: "127.0.0.1:4321".parse().unwrap(),
            owner: "demo".to_owned(),
            tls: Some(TlsConfig {
                certificate: "/work/.devenv/state/mkcert/proxy/cert.pem".into(),
                key: "/work/.devenv/state/mkcert/proxy/key.pem".into(),
            }),
        };
        let decoded: Route = serde_json::from_str(&serde_json::to_string(&route).unwrap()).unwrap();
        assert_eq!(route, decoded);
        let port = proxy_https_listen_address().unwrap().port();
        let expected = if port == 443 {
            "https://docs.demo.localhost".to_owned()
        } else {
            format!("https://docs.demo.localhost:{port}")
        };
        assert_eq!(route_url(&decoded), expected);
    }

    #[tokio::test]
    async fn http_processes_do_not_require_certificate_setup() {
        let mut tasks = vec![process_task("web", &[("http", 8080)])];
        let mut routes = project_routes("demo", "test", &mut tasks).unwrap();
        prepare_https(&mut routes, &mut tasks, &HashMap::new(), None)
            .await
            .unwrap();
        assert!(routes.iter().all(|route| route.tls.is_none()));
        assert_eq!(
            tasks[0].process.as_ref().unwrap().proxy.urls,
            [route_url(&routes[0])]
        );
        assert!(route_url(&routes[0]).starts_with("http://"));
    }

    #[tokio::test]
    async fn https_uses_the_configured_mkcert_without_path_lookup() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let mkcert = directory.path().join("configured-mkcert");
        fs::write(
            &mkcert,
            "#!/bin/sh\nprintf 'configured mkcert invoked' >&2\nexit 23\n",
        )
        .unwrap();
        fs::set_permissions(&mkcert, fs::Permissions::from_mode(0o755)).unwrap();
        // Skip trust installation so the test only exercises tool selection.
        fs::write(directory.path().join("rootCA.pem"), "existing CA").unwrap();
        let envs = HashMap::from([
            (
                "CAROOT".to_owned(),
                directory.path().to_string_lossy().into_owned(),
            ),
            (
                "DEVENV_MKCERT".to_owned(),
                mkcert.to_string_lossy().into_owned(),
            ),
            (
                "PATH".to_owned(),
                directory
                    .path()
                    .join("empty-path")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ]);
        let mut tasks = vec![process_task("web", &[("http", 8080)])];
        tasks[0].process.as_mut().unwrap().proxy.https.enable = true;
        let mut routes = project_routes("demo", "test", &mut tasks).unwrap();
        let error = prepare_https(&mut routes, &mut tasks, &envs, None)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("configured mkcert invoked"),
            "{error:?}"
        );
    }

    #[test]
    fn https_only_selects_routes_of_opted_in_processes() {
        let mut tasks = vec![
            process_task("web", &[("http", 8080)]),
            process_task_with_hostnames(
                "api",
                &[("http", 8081), ("admin", 9000)],
                Some("api.localhost"),
                &[("admin", "control.localhost")],
            ),
            process_task("worker", &[]),
        ];
        tasks[1].process.as_mut().unwrap().proxy.https.enable = true;
        tasks[2].process.as_mut().unwrap().proxy.https.enable = true;
        let mut routes = project_routes("demo", "test", &mut tasks).unwrap();
        // The flag must survive the task configuration passed to the manager.
        let mut tasks: Vec<tasks::TaskConfig> =
            serde_json::from_str(&serde_json::to_string(&tasks).unwrap()).unwrap();
        let selected: std::collections::BTreeSet<_> = https_routes(&mut routes, &tasks)
            .into_iter()
            .map(|route| {
                route.tls = Some(TlsConfig {
                    certificate: "/work/cert.pem".into(),
                    key: "/work/key.pem".into(),
                });
                route.hostname.clone()
            })
            .collect();
        assert_eq!(
            selected,
            ["api.localhost", "http.api.localhost", "control.localhost"]
                .map(str::to_owned)
                .into_iter()
                .collect()
        );
        assert_eq!(routes.len(), 4);
        assert!(
            routes
                .iter()
                .any(|route| route.hostname == "web.demo.localhost")
        );
        update_proxy_urls(&routes, &mut tasks);
        let tasks: Vec<tasks::TaskConfig> =
            serde_json::from_str(&serde_json::to_string(&tasks).unwrap()).unwrap();
        let web = &tasks[0].process.as_ref().unwrap().proxy.urls;
        assert_eq!(web.len(), 1);
        assert!(web[0].starts_with("http://web.demo.localhost"));
        let api = &tasks[1].process.as_ref().unwrap().proxy.urls;
        assert_eq!(api.len(), 3);
        assert!(api.iter().all(|url| url.starts_with("https://")));
        assert!(tasks[2].process.as_ref().unwrap().proxy.urls.is_empty());
    }

    #[tokio::test]
    async fn waits_for_previous_proxy_to_release_control_socket() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("proxy.sock");
        let control = UnixListener::bind(&socket).unwrap();
        control.set_nonblocking(true).unwrap();
        // Reserve the port without serving HTTP, as during daemon shutdown.
        let http = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let listen = http.local_addr().unwrap();
        let started = Instant::now();
        let shutdown_delay = Duration::from_millis(400);
        let old_daemon = std::thread::spawn(move || {
            while started.elapsed() < shutdown_delay {
                match control.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(START_TIMEOUT)).unwrap();
                        let mut line = String::new();
                        BufReader::new(&stream).read_line(&mut line).unwrap();
                        stream
                            .write_all(b"{\"status\":\"ok\",\"routes\":[]}\n")
                            .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("control accept failed: {error}"),
                }
            }
            drop(control);
            drop(http);
        });

        assert!(!wait_for_existing_proxy(&socket, listen).await.unwrap());
        assert!(started.elapsed() >= shutdown_delay);
        old_daemon.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn authentication_requires_frontend_handoff() {
        use std::os::unix::fs::PermissionsExt;

        if nix::unistd::geteuid().is_root() {
            return;
        }

        const CHILD: &str = "DEVENV_TEST_PROXY_FRONTEND_HANDOFF";
        if std::env::var_os(CHILD).is_some() {
            let (frontend, receiver) = tokio::sync::mpsc::channel(1);
            drop(receiver);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let error = runtime
                .block_on(reconcile(
                    "test",
                    vec![Route {
                        hostname: "test.localhost".to_owned(),
                        upstream: "127.0.0.1:8080".parse().unwrap(),
                        owner: "test".to_owned(),
                        tls: None,
                    }],
                    Some(&frontend),
                ))
                .unwrap_err();
            assert!(
                format!("{error:?}")
                    .contains("terminal frontend stopped before sudo authentication"),
                "{error:?}"
            );
            return;
        }

        // Isolate environment overrides and fake sudo in a subprocess. Cached
        // credentials reach the handoff without requiring a terminal or root.
        let directory = tempfile::tempdir().unwrap();
        let sudo = directory.path().join("sudo");
        fs::write(&sudo, "#!/bin/sh\n[ \"$1\" = -n ] && [ \"$2\" = true ]\n").unwrap();
        fs::set_permissions(&sudo, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "proxy::tests::authentication_requires_frontend_handoff",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", directory.path())
            .env("DEVENV_PROXY_BINARY", &sudo)
            .env("DEVENV_PROXY_SOCKET", directory.path().join("proxy.sock"))
            .env("DEVENV_PROXY_LISTEN", "127.0.0.1:80")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn process_task(name: &str, ports: &[(&str, u16)]) -> tasks::TaskConfig {
        process_task_with_hostname(name, ports, None)
    }

    fn process_task_with_hostname(
        name: &str,
        ports: &[(&str, u16)],
        hostname: Option<&str>,
    ) -> tasks::TaskConfig {
        process_task_with_hostnames(name, ports, hostname, &[])
    }

    fn process_task_with_hostnames(
        name: &str,
        ports: &[(&str, u16)],
        hostname: Option<&str>,
        port_hostnames: &[(&str, &str)],
    ) -> tasks::TaskConfig {
        tasks::TaskConfig {
            name: format!("{}{}", devenv_tasks::PROCESS_TASK_PREFIX, name),
            process: Some(ProcessConfig {
                ports: ports
                    .iter()
                    .map(|(name, port)| ((*name).to_owned(), *port))
                    .collect(),
                proxy: ProcessProxyConfig {
                    hostname: hostname.map(str::to_owned),
                    port_hostnames: port_hostnames
                        .iter()
                        .map(|(port, hostname)| ((*port).to_owned(), (*hostname).to_owned()))
                        .collect(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn one_port_uses_process_and_project_names() {
        let routes = project_routes(
            "my_project",
            "/work/my-project",
            &mut [process_task("web_app", &[("server", 8080)])],
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "web-app.my-project.localhost");
        assert_eq!(routes[0].upstream.port(), 8080);
    }

    #[test]
    fn process_urls_match_registered_routes_and_survive_serialization() {
        let mut tasks = vec![
            process_task("docs", &[("http", 4321)]),
            process_task_with_hostnames(
                "api",
                &[("http", 8080), ("admin", 9000)],
                Some("api.localhost"),
                &[("admin", "admin.localhost")],
            ),
            process_task("worker", &[]),
        ];
        let name = project_name(None, Path::new("/work/devenv8")).unwrap();
        let routes = project_routes(&name, "test", &mut tasks).unwrap();
        assert_eq!(routes[0].hostname, "docs.devenv8.localhost");

        // Daemon startup serializes task configs before the process manager
        // creates the activities replayed to attached clients.
        let json = serde_json::to_string(&tasks).unwrap();
        let tasks: Vec<tasks::TaskConfig> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            tasks[0].process.as_ref().unwrap().proxy.urls,
            [route_url(&routes[0])]
        );
        assert_eq!(
            tasks[1].process.as_ref().unwrap().proxy.urls,
            routes[1..].iter().map(route_url).collect::<Vec<_>>()
        );
        assert!(tasks[2].process.as_ref().unwrap().proxy.urls.is_empty());
    }

    #[test]
    fn default_project_name_uses_directory_name() {
        assert_eq!(
            project_name(
                Some(DEFAULT_PROJECT_NAME.to_owned()),
                Path::new("/work/my-project")
            )
            .unwrap(),
            "my-project"
        );
        assert_eq!(
            project_name(Some("custom".to_owned()), Path::new("/work/my-project")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn multiple_ports_use_http_as_default_and_expose_named_urls() {
        let routes = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task("web", &[("http", 8080), ("admin", 9000)])],
        )
        .unwrap();
        let hostnames: BTreeMap<_, _> = routes
            .into_iter()
            .map(|route| (route.hostname, route.upstream.port()))
            .collect();
        assert_eq!(hostnames.get("web.demo.localhost"), Some(&8080));
        assert_eq!(hostnames.get("http.web.demo.localhost"), Some(&8080));
        assert_eq!(hostnames.get("admin.web.demo.localhost"), Some(&9000));
    }

    #[test]
    fn multiple_ports_without_http_have_only_named_urls() {
        let routes = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task("web", &[("public", 8080), ("admin", 9000)])],
        )
        .unwrap();
        assert_eq!(routes.len(), 2);
        assert!(
            routes
                .iter()
                .any(|route| route.hostname == "public.web.demo.localhost")
        );
    }

    #[test]
    fn process_hostname_overrides_generated_hostname() {
        let routes = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task_with_hostname(
                "web",
                &[("http", 8080), ("admin", 9000)],
                Some("APP.Localhost."),
            )],
        )
        .unwrap();
        let hostnames: BTreeMap<_, _> = routes
            .into_iter()
            .map(|route| (route.hostname, route.upstream.port()))
            .collect();
        assert_eq!(hostnames.get("app.localhost"), Some(&8080));
        assert_eq!(hostnames.get("http.app.localhost"), Some(&8080));
        assert_eq!(hostnames.get("admin.app.localhost"), Some(&9000));
    }

    #[test]
    fn process_hostname_override_must_be_localhost() {
        let error = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task_with_hostname(
                "web",
                &[("http", 8080)],
                Some("example.com"),
            )],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid proxy hostname for process web")
        );
    }

    #[test]
    fn port_hostname_overrides_process_and_generated_hostnames() {
        let routes = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task_with_hostnames(
                "web",
                &[("http", 8080), ("admin", 9000), ("metrics", 9001)],
                Some("app.localhost"),
                &[("http", "public.localhost"), ("admin", "control.localhost")],
            )],
        )
        .unwrap();
        let hostnames: BTreeMap<_, _> = routes
            .into_iter()
            .map(|route| (route.hostname, route.upstream.port()))
            .collect();
        assert_eq!(hostnames.get("public.localhost"), Some(&8080));
        assert_eq!(hostnames.get("control.localhost"), Some(&9000));
        assert_eq!(hostnames.get("metrics.app.localhost"), Some(&9001));
        assert_eq!(hostnames.len(), 3);
    }

    #[test]
    fn port_hostname_override_must_be_localhost() {
        let error = project_routes(
            "demo",
            "/work/demo",
            &mut [process_task_with_hostnames(
                "web",
                &[("http", 8080)],
                None,
                &[("http", "example.com")],
            )],
        )
        .unwrap_err();
        assert!(error.to_string().contains("port http of process web"));
    }
}
