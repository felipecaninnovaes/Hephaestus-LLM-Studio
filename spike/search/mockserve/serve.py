"""Embedder mock do spike 3f.0 — stdlib puro, sem torch.

Contrato (fixado pela ADR-0004 D1, algoritmo igual ao MockEmbedder Rust):
  GET  /health      -> {"status":"ok","mode":"mock","dim":512}
  POST /embed       {"model":str,"items":[{"id":any,"b64":str}]}
                    -> {"items":[{"id":any,"vector":[f64]*512,"dim":512}]}
  POST /embed-text  {"model":str,"texts":[str]}
                    -> {"items":[{"id":índice,"vector":[f64]*512,"dim":512}]}
Vetor mock: sha256-chain do payload -> 512 f64 em [-1,1) -> normalização L2.
"""

import base64
import hashlib
import json
import math
import struct
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DIM = 512


def mock_vector(payload: bytes) -> list:
    h = hashlib.sha256(payload).digest()
    vals = []
    while len(vals) < DIM:
        h = hashlib.sha256(h).digest()
        for i in range(4):
            q = struct.unpack_from("<Q", h, i * 8)[0] & ((1 << 53) - 1)
            vals.append((q / float(1 << 53)) * 2.0 - 1.0)
    n = math.sqrt(sum(x * x for x in vals))
    return [x / n for x in vals]


class Handler(BaseHTTPRequestHandler):
    def _json(self, code: int, obj) -> None:
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args) -> None:
        pass

    def do_GET(self) -> None:
        if self.path == "/health":
            self._json(200, {"status": "ok", "mode": "mock", "dim": DIM})
        else:
            self._json(404, {"error": "not_found"})

    def do_POST(self) -> None:
        try:
            length = int(self.headers.get("Content-Length", "0"))
            data = json.loads(self.rfile.read(length))
            if self.path == "/embed":
                items = [
                    {
                        "id": it["id"],
                        "vector": mock_vector(base64.b64decode(it["b64"])),
                        "dim": DIM,
                    }
                    for it in data["items"]
                ]
            elif self.path == "/embed-text":
                items = [
                    {"id": i, "vector": mock_vector(t.encode("utf-8")), "dim": DIM}
                    for i, t in enumerate(data["texts"])
                ]
            else:
                self._json(404, {"error": "not_found"})
                return
            self._json(200, {"items": items})
        except Exception as exc:  # noqa: BLE001
            self._json(400, {"error": str(exc)})


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8090), Handler).serve_forever()
