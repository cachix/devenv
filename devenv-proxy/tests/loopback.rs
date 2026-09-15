#![cfg(unix)]

use devenv_proxy::{ControlRequest, Route, request};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const HOSTNAME: &str = "docs.test.localhost";
const BODY: &str = "loopback request body";

struct Proxy {
    child: Child,
    address: SocketAddr,
    https_address: Option<SocketAddr>,
    directory: tempfile::TempDir,
}

impl Proxy {
    fn start() -> Self {
        Self::start_with_https(false)
    }

    fn start_with_https(https: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let https_address = https.then(|| {
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .unwrap()
                .local_addr()
                .unwrap()
        });
        let mut command = Command::new(env!("CARGO_BIN_EXE_devenv-proxy"));
        command
            .args(["--listen", &address.to_string(), "--control-socket"])
            .arg(directory.path().join("proxy.sock"));
        if let Some(address) = https_address {
            command.args(["--https-listen", &address.to_string()]);
        }
        let child = command.spawn().unwrap();
        let mut proxy = Self {
            child,
            address,
            https_address,
            directory,
        };
        proxy.wait_until_ready();
        proxy
    }

    fn wait_until_ready(&mut self) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(self.child.try_wait().unwrap().is_none(), "proxy exited");
            if self.directory.path().join("proxy.sock").exists()
                && TcpStream::connect_timeout(&self.address, TIMEOUT).is_ok()
            {
                return;
            }
            assert!(Instant::now() < deadline, "proxy did not start");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn send(&self, upstream: SocketAddr) -> TcpStream {
        request(
            &self.directory.path().join("proxy.sock"),
            &ControlRequest::Register {
                route: Route {
                    hostname: HOSTNAME.to_owned(),
                    upstream,
                    owner: "test".to_owned(),
                    tls: None,
                },
            },
        )
        .unwrap()
        .into_result()
        .unwrap();
        let mut stream = TcpStream::connect_timeout(&self.address, TIMEOUT).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        write!(
            stream,
            "POST / HTTP/1.1\r\nHost: {HOSTNAME}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{BODY}",
            BODY.len()
        )
        .unwrap();
        stream
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Keep both families bound so an unrelated server cannot claim the unused
// address. A bound socket that isn't listening still refuses connections.
fn loopback_sockets() -> (Socket, Socket, u16) {
    let ipv4 = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    ipv4.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())
        .unwrap();
    let port = ipv4.local_addr().unwrap().as_socket().unwrap().port();
    let ipv6 = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();
    ipv6.set_only_v6(true).unwrap();
    ipv6.bind(&SocketAddr::from((Ipv6Addr::LOCALHOST, port)).into())
        .unwrap();
    (ipv4, ipv6, port)
}

fn respond(listener: &TcpListener) {
    respond_for(listener, HOSTNAME, "http");
}

fn respond_for(listener: &TcpListener, hostname: &str, scheme: &str) {
    let deadline = Instant::now() + TIMEOUT;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "upstream received no request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("upstream accept failed: {error}"),
        }
    };
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(headers.starts_with("POST / HTTP/1.1\r\n"));
    assert!(
        headers
            .to_ascii_lowercase()
            .contains(&format!("host: {hostname}\r\n"))
    );
    assert!(
        headers
            .to_ascii_lowercase()
            .contains(&format!("x-forwarded-proto: {scheme}\r\n")),
        "{headers}"
    );
    let mut body = vec![0; BODY.len()];
    reader.read_exact(&mut body).unwrap();
    assert_eq!(body, BODY.as_bytes());
    reader
        .get_mut()
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .unwrap();
}

#[test]
fn proxies_to_either_loopback_family_and_prefers_the_registered_address() {
    let proxy = Proxy::start();
    for (register_ipv6, listen_ipv4, listen_ipv6) in [
        (false, true, false),
        (false, false, true),
        (true, true, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let (ipv4, ipv6, port) = loopback_sockets();
        if listen_ipv4 {
            ipv4.listen(8).unwrap();
        }
        if listen_ipv6 {
            ipv6.listen(8).unwrap();
        }
        let ipv4: TcpListener = ipv4.into();
        let ipv6: TcpListener = ipv6.into();
        ipv4.set_nonblocking(true).unwrap();
        ipv6.set_nonblocking(true).unwrap();
        let upstream = if register_ipv6 {
            SocketAddr::from((Ipv6Addr::LOCALHOST, port))
        } else {
            SocketAddr::from((Ipv4Addr::LOCALHOST, port))
        };
        let mut client = proxy.send(upstream);
        let expected_ipv6 = listen_ipv6 && (register_ipv6 || !listen_ipv4);
        respond(if expected_ipv6 { &ipv6 } else { &ipv4 });
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("\r\n\r\nok"), "{response}");
        if listen_ipv4 && listen_ipv6 {
            let unused = if expected_ipv6 { &ipv4 } else { &ipv6 };
            assert_eq!(
                unused.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    }
}

#[test]
fn returns_502_when_neither_loopback_address_is_listening() {
    let proxy = Proxy::start();
    let (_ipv4, _ipv6, port) = loopback_sockets();
    let mut client = proxy.send(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
}

#[test]
fn sigterm_releases_the_daemon_for_restart() {
    let mut proxy = Proxy::start();
    assert!(
        Command::new("kill")
            .args(["-TERM", &proxy.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = proxy.child.try_wait().unwrap() {
            assert!(status.success(), "proxy shutdown failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "proxy did not finish shutdown");
        thread::sleep(Duration::from_millis(10));
    }

    // Reuse both addresses, including the stale Unix socket left on disk.
    proxy.child = Command::new(env!("CARGO_BIN_EXE_devenv-proxy"))
        .args(["--listen", &proxy.address.to_string(), "--control-socket"])
        .arg(proxy.directory.path().join("proxy.sock"))
        .spawn()
        .unwrap();
    proxy.wait_until_ready();
    let (ipv4, _ipv6, port) = loopback_sockets();
    ipv4.listen(8).unwrap();
    let listener: TcpListener = ipv4.into();
    listener.set_nonblocking(true).unwrap();
    let mut client = proxy.send(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    respond(&listener);
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

mod certificates;

fn tls_client(
    proxy: &Proxy,
    hostname: &str,
    ca: &openssl::x509::X509,
) -> openssl::ssl::SslStream<TcpStream> {
    let mut connector =
        openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()).unwrap();
    connector.cert_store_mut().add_cert(ca.clone()).unwrap();
    let stream = TcpStream::connect_timeout(&proxy.https_address.unwrap(), TIMEOUT).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    connector.build().connect(hostname, stream).unwrap()
}

fn verify_https(proxy: &Proxy, listener: &TcpListener, hostname: &str, ca: &openssl::x509::X509) {
    let mut client = tls_client(proxy, hostname, ca);
    write!(client, "POST / HTTP/1.1\r\nHost: {hostname}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{BODY}", BODY.len()).unwrap();
    respond_for(listener, hostname, "https");
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

#[test]
fn https_selects_project_certificates_and_reloads_routes() {
    let proxy = Proxy::start_with_https(true);
    let directory = tempfile::tempdir().unwrap();
    let (first, first_ca) = certificates::generate(directory.path(), HOSTNAME);
    let other_hostname = "api.other.localhost";
    let (second, second_ca) = certificates::generate(directory.path(), other_hostname);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let socket = proxy.directory.path().join("proxy.sock");
    let first_route = Route {
        hostname: HOSTNAME.to_owned(),
        upstream: listener.local_addr().unwrap(),
        owner: "first".to_owned(),
        tls: Some(first.clone()),
    };
    let second_route = Route {
        hostname: other_hostname.to_owned(),
        owner: "second".to_owned(),
        tls: Some(second.clone()),
        ..first_route.clone()
    };
    for route in [&first_route, &second_route] {
        request(
            &socket,
            &ControlRequest::Register {
                route: route.clone(),
            },
        )
        .unwrap()
        .into_result()
        .unwrap();
    }
    verify_https(&proxy, &listener, HOSTNAME, &first_ca);
    verify_https(&proxy, &listener, other_hostname, &second_ca);

    // Reject a certificate for another hostname without replacing a valid route.
    let wrong_certificate = Route {
        tls: Some(second),
        ..first_route.clone()
    };
    assert!(
        request(
            &socket,
            &ControlRequest::ReplaceOwner {
                owner: "first".to_owned(),
                routes: vec![wrong_certificate],
            }
        )
        .unwrap()
        .into_result()
        .is_err()
    );
    verify_https(&proxy, &listener, HOSTNAME, &first_ca);

    let replacement_directory = tempfile::tempdir().unwrap();
    let (replacement, replacement_ca) =
        certificates::generate(replacement_directory.path(), HOSTNAME);
    request(
        &socket,
        &ControlRequest::ReplaceOwner {
            owner: "first".to_owned(),
            routes: vec![Route {
                tls: Some(replacement),
                ..first_route
            }],
        },
    )
    .unwrap()
    .into_result()
    .unwrap();
    verify_https(&proxy, &listener, HOSTNAME, &replacement_ca);
    verify_https(&proxy, &listener, other_hostname, &second_ca);

    request(
        &socket,
        &ControlRequest::ReplaceOwner {
            owner: "first".to_owned(),
            routes: vec![],
        },
    )
    .unwrap()
    .into_result()
    .unwrap();
    let mut connector =
        openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()).unwrap();
    connector.cert_store_mut().add_cert(replacement_ca).unwrap();
    let stream = TcpStream::connect_timeout(&proxy.https_address.unwrap(), TIMEOUT).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    assert!(connector.build().connect(HOSTNAME, stream).is_err());
    verify_https(&proxy, &listener, other_hostname, &second_ca);
}

#[test]
fn certificate_reuse_requires_the_same_ca_names_and_private_key() {
    let directory = tempfile::tempdir().unwrap();
    let (certificate, ca) = certificates::generate(directory.path(), HOSTNAME);
    let ca_path = directory.path().join("rootCA.pem");
    std::fs::write(&ca_path, ca.to_pem().unwrap()).unwrap();
    assert!(certificate.is_current(&[HOSTNAME.to_owned()], &ca_path));
    assert!(!certificate.is_current(&["other.localhost".to_owned()], &ca_path));
    let (other, other_ca) = certificates::generate(directory.path(), "other.localhost");
    std::fs::write(&ca_path, other_ca.to_pem().unwrap()).unwrap();
    assert!(!certificate.is_current(&[HOSTNAME.to_owned()], &ca_path));
    std::fs::write(&ca_path, ca.to_pem().unwrap()).unwrap();
    std::fs::copy(other.key, &certificate.key).unwrap();
    assert!(!certificate.is_current(&[HOSTNAME.to_owned()], &ca_path));
}
