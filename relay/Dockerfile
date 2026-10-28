# The Puppet Master team relay (relay/server) in a container.
#
#   docker build -t pm-relay relay          # from the repository root
#   docker run -d --name pm-relay --restart unless-stopped -p 127.0.0.1:8787:8787 -v pm-relay-data:/data pm-relay
#
# Extra options go after the image name and are added to the ones below, for example behind a reverse
# proxy on the same machine:   docker run ... pm-relay --trusted-proxy 172.16.0.0/12
#
# Put TLS in front of it (a reverse proxy, or a tunnel): the app only accepts plain http:// for a relay on
# the same computer. The mailbox lives in /data/relay.sqlite (the "pm-relay-data" volume); it runs as an
# unprivileged user (uid 10001), so a folder you mount on /data must be writable by that uid.
#
# The base images are pinned to a release; to pin them byte for byte, add @sha256:<digest> after the tag.
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
COPY protocol protocol
COPY server server
COPY worker/wasm worker/wasm
COPY worker/LICENSE worker/LICENSE
COPY viewer/dist viewer/dist
COPY Cargo.toml Cargo.lock LICENSE ./
RUN cargo build --release --locked -p pm-relay

FROM debian:bookworm-slim
# tini: the relay runs as a plain process, and a container's first process ignores SIGTERM unless it
# handles it, so `docker stop` would wait 10 seconds and then kill it.
RUN apt-get update \
 && apt-get install -y --no-install-recommends tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin pmrelay \
 && mkdir /data \
 && chown pmrelay:pmrelay /data
COPY --from=build /src/target/release/pm-relay /usr/local/bin/pm-relay
USER pmrelay
# Created and owned above, so a new named volume starts out writable.
VOLUME /data
EXPOSE 8787
# No curl in the image: ask /v1/health over a plain TCP connection with bash.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8787 && printf 'GET /v1/health HTTP/1.0\\r\\n\\r\\n' >&3 && head -c 400 <&3 | grep -q '\"ok\":true'"]
ENTRYPOINT ["tini", "--", "pm-relay", "--listen", "0.0.0.0:8787", "--db", "/data/relay.sqlite"]
