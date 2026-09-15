use anyhow::Result;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(about = "Shared reverse proxy for friendly devenv localhost URLs")]
struct Args {
    #[arg(long, env = "DEVENV_PROXY_LISTEN", default_value = "127.0.0.1:80")]
    listen: SocketAddr,
    #[arg(long, env = "DEVENV_PROXY_HTTPS_LISTEN")]
    https_listen: Option<SocketAddr>,
    #[arg(long, env = "DEVENV_PROXY_SOCKET")]
    control_socket: Option<PathBuf>,
}

fn main() -> Result<()> {
    // Pingora starts listeners on service threads. A bind failure must end the
    // daemon rather than leave a live control socket attached to no data plane.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{info}");
        std::process::exit(1);
    }));
    let args = Args::parse();
    let control_socket = args
        .control_socket
        .unwrap_or_else(devenv_proxy::default_control_socket);
    eprintln!(
        "starting devenv proxy on http://{} (control: {})",
        args.listen,
        control_socket.display()
    );
    if let Some(listen) = args.https_listen {
        eprintln!("starting devenv proxy TLS on https://{listen}");
    }
    devenv_proxy::run_with_https(args.listen, args.https_listen, &control_socket)
}
