"""
Servidor HTTP e endpoints de embeddings (/health, /embed, /embed-text).
"""
from __future__ import annotations

import base64
import binascii
import json
import os
import signal
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

from engine_kit.mock import is_mock
from trainer_clip.mock_embed import mock_vector

DIM = 512
MODEL = os.environ.get("MODEL", "ViT-B-32")
PORT = int(os.environ.get("PORT", "8090"))
MOCK = is_mock()

_REAL: Any = None
_SERVER: ThreadingHTTPServer | None = None


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args: Any) -> None:
        pass

    def _send(self, code: int, obj: dict[str, Any]) -> None:
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path == "/health":
            resp: dict[str, Any] = {
                "status": "ok",
                "mode": "mock" if MOCK else "clip",
                "dim": DIM,
            }
            if not MOCK:
                resp["model"] = MODEL
            self._send(200, resp)
        else:
            self._send(404, {"error": "not_found"})

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw = self.rfile.read(length) if length > 0 else b""
        try:
            body = json.loads(raw.decode("utf-8")) if raw else None
        except Exception as e:
            self._send(400, {"error": f"invalid_json: {e}"})
            return
        if self.path == "/embed":
            self._handle_embed(body)
        elif self.path == "/embed-text":
            self._handle_embed_text(body)
        else:
            self._send(404, {"error": "not_found"})

    def _handle_embed(self, body: Any) -> None:
        try:
            if not isinstance(body, dict):
                raise ValueError("bad_shape")
            model = body.get("model")
            items = body.get("items")
            if not isinstance(model, str) or not isinstance(items, list):
                raise ValueError("bad_shape")
            for it in items:
                if (
                    not isinstance(it, dict)
                    or "id" not in it
                    or not isinstance(it.get("b64"), str)
                ):
                    raise ValueError("bad_shape")
        except ValueError as e:
            self._send(400, {"error": str(e)})
            return
        if not MOCK and body["model"] != MODEL:
            self._send(400, {"error": "model_mismatch"})
            return
        try:
            payloads = [
                base64.b64decode(it["b64"], validate=True) for it in items
            ]
        except (binascii.Error, ValueError) as e:
            self._send(400, {"error": f"bad_b64: {e}"})
            return
        if MOCK:
            out = [
                {"id": it["id"], "vector": mock_vector(b), "dim": DIM}
                for it, b in zip(items, payloads)
            ]
        else:
            from trainer_clip.clip_backend import real_embed_images
            try:
                vecs = real_embed_images(_REAL, payloads)
            except Exception as e:
                self._send(400, {"error": f"encode_failed: {e}"})
                return
            out = [
                {"id": it["id"], "vector": v, "dim": DIM}
                for it, v in zip(items, vecs)
            ]
        self._send(200, {"items": out})

    def _handle_embed_text(self, body: Any) -> None:
        try:
            if not isinstance(body, dict):
                raise ValueError("bad_shape")
            model = body.get("model")
            texts = body.get("texts")
            if not isinstance(model, str) or not isinstance(texts, list):
                raise ValueError("bad_shape")
            for t in texts:
                if not isinstance(t, str):
                    raise ValueError("bad_shape")
        except ValueError as e:
            self._send(400, {"error": str(e)})
            return
        if not MOCK and body["model"] != MODEL:
            self._send(400, {"error": "model_mismatch"})
            return
        if MOCK:
            out = [
                {"id": i, "vector": mock_vector(t.encode("utf-8")), "dim": DIM}
                for i, t in enumerate(texts)
            ]
        else:
            from trainer_clip.clip_backend import real_embed_texts
            try:
                vecs = real_embed_texts(_REAL, texts)
            except Exception as e:
                self._send(400, {"error": f"encode_failed: {e}"})
                return
            out = [
                {"id": i, "vector": v, "dim": DIM}
                for i, v in enumerate(vecs)
            ]
        self._send(200, {"items": out})


def _do_shutdown_graceful() -> None:
    time.sleep(0.1)
    if _SERVER is not None:
        _SERVER.shutdown()


def _sigterm_handler(signum: int, frame: Any) -> None:
    print(f"[trainer-clip] Sinal {signum} recebido — shutdown gracioso.", flush=True)
    threading.Thread(target=_do_shutdown_graceful, daemon=True).start()


def run_clip_server(port: int = PORT, model: str = MODEL, mock: bool = MOCK) -> None:
    global _REAL, _SERVER, MODEL, MOCK
    MODEL = model
    MOCK = mock

    if not MOCK:
        from trainer_clip.clip_backend import load_real_clip
        _REAL = load_real_clip(model)

    if threading.current_thread() is threading.main_thread():
        signal.signal(signal.SIGTERM, _sigterm_handler)
        signal.signal(signal.SIGINT, _sigterm_handler)

    server = ThreadingHTTPServer(("0.0.0.0", port), Handler)
    _SERVER = server
    print(f"[trainer-clip] Servidor iniciado na porta {port} (mock={MOCK}).", flush=True)

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        _SERVER = None
        print("[trainer-clip] Servidor encerrado.", flush=True)
