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

/// The number after a flag, or exit with a message.
fn number<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, flag: &str) -> T {
    args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
        eprintln!("{flag} needs a whole number");
        std::process::exit(2);
    })
}

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
            "--rooms-per-address-per-hour" => config.limits.rooms_per_address_per_hour = number(&mut args, &arg),
            "--max-open-rooms" => config.limits.max_open_rooms = number(&mut args, &arg),
            "--room-frames-per-second" => config.limits.room_frames_per_second = number(&mut args, &arg),
            "--room-frames-per-day" => config.limits.room_frames_per_day = number(&mut args, &arg),
            "--viewer-requests-per-minute" => config.limits.viewer_requests_per_minute = number(&mut args, &arg),
            "--seats-per-address" => config.limits.seats_per_address = number(&mut args, &arg),
            "--room-ring-bytes" => config.limits.room_ring_bytes = number(&mut args, &arg),
            "--global-room-bytes" => config.limits.global_room_bytes = number(&mut args, &arg),
            "-h" | "--help" => {
                println!(
                    "pm-relay [--listen 127.0.0.1:8787] [--db relay.sqlite | --memory] [--trusted-proxy <address or range>]...\n\
                     Live Room limits (defaults): [--rooms-per-address-per-hour 10] [--max-open-rooms 100] \
                     [--room-frames-per-second 2] [--room-frames-per-day 7200] [--viewer-requests-per-minute 120] [--seats-per-address 4] \
                     [--room-ring-bytes 2097152] [--global-room-bytes 268435456]"
                );
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
