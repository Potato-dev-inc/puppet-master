# The Puppet Master team relay (relay/server) in a container.
#
#   docker build -t pm-relay relay          # from the repository root
#   docker run -d --name pm-relay -p 8787:8787 -v pm-relay-data:/data pm-relay
#
# Put TLS in front of it (a reverse proxy, or a tunnel): the app only accepts plain http:// for a relay on
# the same computer. The mailbox lives in /data/relay.sqlite.
FROM rust:1-slim AS build
WORKDIR /src
COPY protocol protocol
COPY server server
COPY worker/wasm worker/wasm
COPY worker/LICENSE worker/LICENSE
COPY Cargo.toml Cargo.lock LICENSE ./
RUN cargo build --release --locked -p pm-relay

FROM debian:stable-slim
COPY --from=build /src/target/release/pm-relay /usr/local/bin/pm-relay
VOLUME /data
EXPOSE 8787
ENTRYPOINT ["pm-relay", "--listen", "0.0.0.0:8787", "--db", "/data/relay.sqlite"]
