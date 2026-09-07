"""Servidor de embeddings do engine trainer-clip (ADR-0004 D1, fatia 3f).

ENGINE_MOCK=1  → vetores hash determinísticos (stdlib puro, SEM importar torch).
Sem ENGINE_MOCK → open_clip_torch ViT-B-32 (laion2b_s34b_b79k), GPU se disponível.
Contrato (fixado pelo spike 3f.0 — idêntico ao MockEmbedder Rust em src/search/embed.rs):
  GET  /health      -> {"status":"ok","mode":"mock|clip","dim":512[,"model":str]}
  POST /embed       {"model":str,"items":[{"id":any,"b64":str}]}
                    -> {"items":[{"id":ecoado,"vector":[f64]*512,"dim":512}]}
  POST /embed-text  {"model":str,"texts":[str]}
                    -> {"items":[{"id":índice,"vector":[f64]*512,"dim":512}]}
"""

import base64
import binascii
import hashlib
import json
import math
import os
import struct
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DIM = 512
MODEL = os.environ.get("MODEL", "ViT-B-32")
PORT = int(os.environ.get("PORT", "8090"))
MOCK = os.environ.get("ENGINE_MOCK") == "1"


def mock_vector(payload: bytes) -> list:
    h = hashlib.sha256(payload).digest()
    vals = []
    while len(vals) < 512:
        h = hashlib.sha256(h).digest()
        for i in range(4):
            q = struct.unpack_from("<Q", h, i * 8)[0] & ((1 << 53) - 1)
            vals.append((q / float(1 << 53)) * 2.0 - 1.0)
    n = math.sqrt(sum(x * x for x in vals))
    return [x / n for x in vals]


# Estado do modo real (preenchido no boot quando não-mock).
_REAL = None


def _load_real():
    import torch
    import open_clip

    device = "cuda" if torch.cuda.is_available() else "cpu"
    model, _, preprocess = open_clip.create_model_and_transforms(
        "ViT-B-32", pretrained="laion2b_s34b_b79k", device=device
    )
    tokenizer = open_clip.get_tokenizer("ViT-B-32")
    model.eval()
    state = {
        "torch": torch,
        "model": model,
        "preprocess": preprocess,
        "tokenizer": tokenizer,
        "device": device,
    }
    return state


def _real_embed_images(payloads: list) -> list:
    import io

    from PIL import Image

    torch = _REAL["torch"]
    model = _REAL["model"]
    preprocess = _REAL["preprocess"]
    device = _REAL["device"]
    imgs = []
    for b in payloads:
        img = Image.open(io.BytesIO(b)).convert("RGB")
        imgs.append(preprocess(img))
    batch = torch.stack(imgs).to(device)
    with torch.no_grad():
        feats = model.encode_image(batch)
        feats = torch.nn.functional.normalize(feats, dim=-1)
    return feats.cpu().tolist()


def _real_embed_texts(texts: list) -> list:
    torch = _REAL["torch"]
    model = _REAL["model"]
    tokenizer = _REAL["tokenizer"]
    device = _REAL["device"]
    tokens = tokenizer(texts).to(device)
    with torch.no_grad():
        feats = model.encode_text(tokens)
        feats = torch.nn.functional.normalize(feats, dim=-1)
    return feats.cpu().tolist()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _send(self, code: int, obj: dict):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            resp = {"status": "ok", "mode": "mock" if MOCK else "clip", "dim": DIM}
            if not MOCK:
                resp["model"] = MODEL
            self._send(200, resp)
        else:
            self._send(404, {"error": "not_found"})

    def do_POST(self):
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

    def _handle_embed(self, body):
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
            try:
                vecs = _real_embed_images(payloads)
            except Exception as e:
                self._send(400, {"error": f"encode_failed: {e}"})
                return
            out = [
                {"id": it["id"], "vector": v, "dim": DIM}
                for it, v in zip(items, vecs)
            ]
        self._send(200, {"items": out})

    def _handle_embed_text(self, body):
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
            try:
                vecs = _real_embed_texts(texts)
            except Exception as e:
                self._send(400, {"error": f"encode_failed: {e}"})
                return
            out = [
                {"id": i, "vector": v, "dim": DIM}
                for i, v in enumerate(vecs)
            ]
        self._send(200, {"items": out})


def main() -> None:
    global _REAL
    if not MOCK:
        _REAL = _load_real()
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
