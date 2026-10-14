//! `pm-relay`: run the team relay.
//!
//! ```text
//! pm-relay [--listen 127.0.0.1:8787] [--db relay.sqlite]
//! ```
//!
//! Put it behind TLS (a reverse proxy, or a tunnel) when teammates reach it over the internet:
//! Puppet Master refuses plain `http://` relays that are not on the same computer.

use pm_relay::{serve, Config, Relay};

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
            "-h" | "--help" => {
                println!("pm-relay [--listen 127.0.0.1:8787] [--db relay.sqlite | --memory]");
                return;
            }
            other => {
                eprintln!("unknown argument {other:?}; try --help");
                std::process::exit(2);
            }
        }
    }
    let relay = Relay::open(&config).expect("open the relay store");
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind the listen address");
        tracing::info!(%listen, protocol = pm_relay_protocol::PROTOCOL, "relay: listening");
        serve(listener, relay).await.expect("serve");
    });
}
