//! Embedded HTTP server for the mobile PWA (production builds).
//!
//! Serves the staged `pwa-dist` assets on loopback, proxies `/bridge/*` to the
//! embedded bridge, and exposes `/__puppet_master_dev__.json` for tunnel info.
#![allow(dead_code)]

use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

const HOST: &str = "127.0.0.1";
pub const PWA_PORT: u16 = 1420;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevInfoPayload {
    pub local_url: String,
    pub pwa_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_provider: Option<String>,
    pub bridge_proxy_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_direct_url: Option<String>,
    pub updated_at: u64,
}

type DevInfoSupplier = Arc<dyn Fn() -> DevInfoPayload + Send + Sync>;

pub struct PwaServerHandle {
    _join: thread::JoinHandle<()>,
}

pub fn start_pwa_server(
    dist_root: PathBuf,
    bridge_port_file: PathBuf,
    dev_info: DevInfoSupplier,
) -> Result<PwaServerHandle, String> {
    if !dist_root.is_dir() {
        return Err(format!(
            "PWA dist directory missing: {}",
            dist_root.display()
        ));
    }

    let listener = TcpListener::bind((HOST, PWA_PORT))
        .map_err(|err| format!("bind PWA server on {HOST}:{PWA_PORT}: {err}"))?;

    let join = thread::Builder::new()
        .name("puppet-master-pwa-server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let dist_root = dist_root.clone();
                        let bridge_port_file = bridge_port_file.clone();
                        let dev_info = dev_info.clone();
                        thread::spawn(move || {
                            if let Err(err) =
                                handle_connection(stream, &dist_root, &bridge_port_file, &dev_info)
                            {
                                tracing::debug!(%err, "PWA request failed");
                            }
                        });
                    }
                    Err(err) => tracing::debug!(%err, "PWA accept failed"),
                }
            }
        })
        .map_err(|err| format!("spawn PWA server thread: {err}"))?;

    Ok(PwaServerHandle { _join: join })
}

fn handle_connection(
    mut stream: TcpStream,
    dist_root: &Path,
    bridge_port_file: &Path,
    dev_info: &DevInfoSupplier,
) -> Result<(), String> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end;
    loop {
        let n = stream
            .read(&mut chunk)
            .map_err(|err| format!("read request: {err}"))?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
        if buf.len() > 1024 * 1024 {
            return Err("request headers too large".into());
        }
    }

    let headers_raw = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = headers_raw.lines();
    let request_line = lines.next().ok_or("empty request")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");
    let (path, _query) = split_target(target);

    let content_length = parse_content_length(&headers_raw);
    let body_start = header_end + 4;
    while buf.len().saturating_sub(body_start) < content_length {
        let n = stream
            .read(&mut chunk)
            .map_err(|err| format!("read body: {err}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[body_start..body_start.saturating_add(content_length).min(buf.len())];

    if path == "/__puppet_master_dev__.json" && method == "GET" {
        let payload = dev_info();
        return write_json(&mut stream, 200, &payload);
    }

    if path == "/bridge" || path.starts_with("/bridge/") {
        let bridge_path = path.strip_prefix("/bridge").unwrap_or("");
        let bridge_path = if bridge_path.is_empty() {
            "/"
        } else {
            bridge_path
        };
        return proxy_bridge(
            &mut stream,
            bridge_port_file,
            method,
            bridge_path,
            &headers_raw,
            body,
        );
    }

    serve_static(&mut stream, dist_root, path, method)
}

fn looks_like_asset_path(path: &str) -> bool {
    let rel = path.trim_start_matches('/');
    if rel.is_empty() {
        return false;
    }
    if rel.starts_with("assets/") || rel.starts_with("@") || rel.starts_with("src/") {
        return true;
    }
    rel.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext,
            "js" | "mjs"
                | "ts"
                | "tsx"
                | "css"
                | "map"
                | "json"
                | "wasm"
                | "svg"
                | "png"
                | "ico"
                | "woff2"
        )
    })
}

fn serve_static(
    stream: &mut TcpStream,
    dist_root: &Path,
    path: &str,
    method: &str,
) -> Result<(), String> {
    if method != "GET" && method != "HEAD" {
        return write_text(stream, 405, "Method Not Allowed");
    }

    let rel = path.trim_start_matches('/');
    let mut file_path = if rel.is_empty() {
        dist_root.join("index.html")
    } else {
        dist_root.join(rel)
    };

    if file_path.is_dir() {
        file_path = file_path.join("index.html");
    }

    if !file_path.exists() {
        if looks_like_asset_path(path) {
            return write_text(stream, 404, "Not Found");
        }
        file_path = dist_root.join("index.html");
    }

    let canonical_root = dist_root
        .canonicalize()
        .map_err(|err| format!("canonicalize dist root: {err}"))?;
    let canonical_file = file_path
        .canonicalize()
        .map_err(|_| "not found".to_string())?;
    if !canonical_file.starts_with(&canonical_root) {
        return write_text(stream, 403, "Forbidden");
    }

    let bytes = fs::read(&canonical_file).map_err(|_| "not found".to_string())?;
    let content_type = content_type_for(&canonical_file);
    write_bytes(stream, 200, content_type, &bytes, method == "HEAD")
}

fn proxy_bridge(
    stream: &mut TcpStream,
    bridge_port_file: &Path,
    method: &str,
    path: &str,
    headers_raw: &str,
    body: &[u8],
) -> Result<(), String> {
    let bridge_addr = read_bridge_addr(bridge_port_file);
    let mut upstream = TcpStream::connect(&bridge_addr)
        .map_err(|err| format!("connect bridge {bridge_addr}: {err}"))?;

    let mut request = format!("{method} {path} HTTP/1.1\r\n");
    for line in headers_raw.lines().skip(1) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("host:")
            || lower.starts_with("connection:")
            || lower.starts_with("proxy-connection:")
            || lower.starts_with("content-length:")
        {
            continue;
        }
        request.push_str(line);
        request.push_str("\r\n");
    }
    request.push_str(&format!("Host: {bridge_addr}\r\n"));
    request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    request.push_str("Connection: close\r\n\r\n");
    upstream
        .write_all(request.as_bytes())
        .map_err(|err| format!("write bridge request: {err}"))?;
    if !body.is_empty() {
        upstream
            .write_all(body)
            .map_err(|err| format!("write bridge body: {err}"))?;
    }

    let mut response = Vec::new();
    upstream
        .read_to_end(&mut response)
        .map_err(|err| format!("read bridge response: {err}"))?;
    stream
        .write_all(&response)
        .map_err(|err| format!("write proxy response: {err}"))?;
    Ok(())
}

fn read_bridge_addr(bridge_port_file: &Path) -> String {
    match fs::read_to_string(bridge_port_file) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.contains(':') {
                format!("http://{trimmed}")
            } else if !trimmed.is_empty() {
                format!("http://127.0.0.1:{trimmed}")
            } else {
                "http://127.0.0.1:17321".into()
            }
        }
        Err(_) => "http://127.0.0.1:17321".into(),
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

fn parse_content_length(headers: &str) -> usize {
    for line in headers.lines().skip(1) {
        if let Some(value) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
        {
            if let Ok(n) = value.trim().parse::<usize>() {
                return n;
            }
        }
    }
    0
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn write_json(stream: &mut TcpStream, status: u16, value: &impl Serialize) -> Result<(), String> {
    let body = serde_json::to_string(value).map_err(|err| format!("json encode: {err}"))?;
    write_text_body(stream, status, "application/json; charset=utf-8", &body)
}

fn write_text(stream: &mut TcpStream, status: u16, message: &str) -> Result<(), String> {
    write_text_body(stream, status, "text/plain; charset=utf-8", message)
}

fn write_text_body(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    write_bytes(stream, status, content_type, body.as_bytes(), false)
}

fn write_bytes(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| format!("write headers: {err}"))?;
    if !head_only {
        stream
            .write_all(body)
            .map_err(|err| format!("write body: {err}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bridge_addr_parses_host_port_file() {
        let dir = std::env::temp_dir().join(format!("pm-pwa-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("port");
        fs::write(&file, "127.0.0.1:17355\n").unwrap();
        assert_eq!(read_bridge_addr(&file), "http://127.0.0.1:17355");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn looks_like_asset_path_detects_js_and_vite_paths() {
        assert!(looks_like_asset_path("/src/main.tsx"));
        assert!(looks_like_asset_path("/assets/index.js"));
        assert!(looks_like_asset_path("/@vite/client"));
        assert!(!looks_like_asset_path("/pair/invite"));
    }
}
