//! Automatic integration with the shared `devenv-proxy` daemon.

use crate::tasks;
use devenv_mailbox::FrontendCommand;
use devenv_proxy::{ControlRequest, ControlResponse, Route};
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
    let port = proxy_listen_address()
        .map(|address| address.port())
        .unwrap_or(80);
    if port == 80 {
        format!("http://{}", route.hostname)
    } else {
        format!("http://{}:{port}", route.hostname)
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

    ensure_running(frontend).await?;
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
) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    let _ = frontend;

    let socket = devenv_proxy::default_control_socket();
    let listen = proxy_listen_address()
        .ok_or_else(|| miette!("DEVENV_PROXY_LISTEN is not a valid socket address"))?;
    if wait_for_existing_proxy(&socket, listen).await? {
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
    let args = vec![
        "--listen".to_owned(),
        listen.to_string(),
        "--control-socket".to_owned(),
        socket
            .to_str()
            .ok_or_else(|| miette!("proxy control socket path is not valid UTF-8"))?
            .to_owned(),
    ];

    #[cfg(target_os = "linux")]
    let detached_pid = if listen.port() < 1024 && !nix::unistd::geteuid().is_root() {
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
    i32::try_from(pid)
        .is_ok_and(|pid| nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok())
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
