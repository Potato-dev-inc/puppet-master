"""Puppet Master docs site on :5000, plus GET /download (zip of the markdown)."""
from __future__ import annotations

import io
import zipfile
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PORT = 5000
ZIP_NAME = "puppet-master-docs.zip"
DOWNLOAD_PATHS = {"/download", "/docs.zip"}


def md_files() -> list[Path]:
    return sorted(p for p in ROOT.rglob("*.md") if p.is_file())


def zip_docs() -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        for path in md_files():
            zf.write(path, path.relative_to(ROOT).as_posix())
    return buf.getvalue()


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)

    def do_GET(self):
        route = self.path.split("?", 1)[0].rstrip("/") or "/"
        if route in DOWNLOAD_PATHS or route == "/download":
            data = zip_docs()
            self.send_response(200)
            self.send_header("Content-Type", "application/zip")
            self.send_header("Content-Disposition", f'attachment; filename="{ZIP_NAME}"')
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(data)
            return
        super().do_GET()

    def log_message(self, format, *args):
        print("[%s] %s" % (self.log_date_time_string(), format % args))


if __name__ == "__main__":
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    print(f"docs  http://localhost:{PORT}", flush=True)
    print(f"zip   http://localhost:{PORT}/download", flush=True)
    server.serve_forever()
