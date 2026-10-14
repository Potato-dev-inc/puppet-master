//! `pm-relay`: run the team relay.
//!
//! ```text
//! pm-relay [--listen 127.0.0.1:8787] [--db relay.sqlite] [--trusted-proxy 127.0.0.1 ...]
//! ```
//!
//! Put it behind TLS (a reverse proxy, or a tunnel) when teammates reach it over the internet:
//! Puppet Master refuses plain `http://` relays that are not on the same computer. Behind a proxy,
//! pass `--trusted-proxy <address or range>` (repeatable) so per-address limits count each client,
//! not the proxy: `X-Forwarded-For` is read only from those addresses.

use pm_relay::{serve, Cidr, Config, Relay};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let mut listen = "127.0.0.1:8787".to_string();
    let mut config = Config { db: Some("relay.sqlite".into()), ..Config::default() };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().expect("--listen needs an address"),
            "--db" => config.db = Some(args.next().expect("--db needs a path").into()),
            "--memory" => config.db = None,
            "--trusted-proxy" => {
                let range = args.next().expect("--trusted-proxy needs an address or range");
                match Cidr::parse(&range) {
                    Ok(cidr) => config.trusted_proxies.push(cidr),
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(2);
                    }
                }
            }
            "-h" | "--help" => {
                println!("pm-relay [--listen 127.0.0.1:8787] [--db relay.sqlite | --memory] [--trusted-proxy <address or range>]...");
                return;
            }
            other => {
                eprintln!("unknown argument {other:?}; try --help");
                std::process::exit(2);
            }
        }
    }
    if config.trusted_proxies.is_empty() {
        tracing::info!("relay: no --trusted-proxy; per-address limits count the TCP peer (behind a reverse proxy, that is the proxy)");
    }
    let relay = Relay::open(&config).expect("open the relay store");
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind the listen address");
        tracing::info!(%listen, protocol = pm_relay_protocol::PROTOCOL, "relay: listening");
        serve(listener, relay).await.expect("serve");
    });
}
