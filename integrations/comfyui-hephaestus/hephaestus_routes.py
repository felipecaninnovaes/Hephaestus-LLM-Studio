"""Rotas HTTP do Hephaestus para o ComfyUI: upload de LoRA em partes.

Este módulo não depende do ComfyUI: `build_routes` recebe provedores de token e
de diretório, o que permite testá-lo com `aiohttp.test_utils`.
"""

from __future__ import annotations

import asyncio
import hashlib
import hmac
import json
import os
import re
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Awaitable, Callable, Optional

from aiohttp import web

VERSION = "1"
CHUNK_SIZE = 32 * 1024 * 1024
STALE_SECONDS = 3600
SUBDIR = "hephaestus"
UPLOADS_DIR = ".uploads"
_READ_BLOCK = 1024 * 1024
_FILENAME_RE = re.compile(r"^[A-Za-z0-9._-]+$")


def valid_filename(name: object) -> bool:
    return (
        isinstance(name, str)
        and 0 < len(name) <= 200
        and name.endswith(".safetensors")
        and ".." not in name
        and _FILENAME_RE.fullmatch(name) is not None
    )


def _err(status: int, code: str, message: str, **extra: object) -> web.Response:
    return web.json_response({"error": code, "message": message, **extra}, status=status)


@dataclass
class _Upload:
    filename: str
    size: int
    overwrite: bool
    part: Path
    final: Path
    lock: asyncio.Lock = field(default_factory=asyncio.Lock)


def _append(part: Path, data: bytes) -> None:
    with open(part, "ab") as f:
        f.write(data)


def _truncate(part: Path, length: int) -> None:
    try:
        os.truncate(part, length)
    except FileNotFoundError:
        pass


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while block := f.read(_READ_BLOCK):
            h.update(block)
    return h.hexdigest()


def _cleanup_stale(uploads_dir: Path, now: float) -> None:
    if not uploads_dir.is_dir():
        return
    for p in uploads_dir.glob("*.part"):
        try:
            if now - p.stat().st_mtime > STALE_SECONDS:
                p.unlink()
        except OSError:
            pass


def build_routes(
    get_token: Callable[[], Optional[str]],
    get_lora_dir: Callable[[], Path],
) -> web.RouteTableDef:
    routes = web.RouteTableDef()
    uploads: dict[str, _Upload] = {}

    def authorized(handler: Callable[[web.Request], Awaitable[web.StreamResponse]]):
        async def wrapper(request: web.Request) -> web.StreamResponse:
            token = get_token()
            if not token:
                return _err(503, "token_not_configured", "Token do Hephaestus não configurado no ComfyUI.")
            header = request.headers.get("Authorization", "")
            scheme, _, given = header.partition(" ")
            if scheme.lower() != "bearer" or not hmac.compare_digest(
                given.strip().encode(), token.encode()
            ):
                return _err(401, "unauthorized", "Token inválido.")
            return await handler(request)

        return wrapper

    def dest_dir() -> Path:
        return Path(get_lora_dir()) / SUBDIR

    def lookup(request: web.Request) -> tuple[Optional[str], Optional[_Upload]]:
        uid = request.match_info["upload_id"]
        return uid, uploads.get(uid)

    def _not_found() -> web.Response:
        return _err(404, "upload_not_found", "Upload não encontrado.")

    @routes.get("/hephaestus/health")
    @authorized
    async def health(request: web.Request) -> web.StreamResponse:
        return web.json_response(
            {"version": VERSION, "chunkSize": CHUNK_SIZE, "loraDir": str(get_lora_dir().resolve())}
        )

    @routes.post("/hephaestus/lora/uploads")
    @authorized
    async def init_upload(request: web.Request) -> web.StreamResponse:
        try:
            body = await request.json()
        except (json.JSONDecodeError, UnicodeDecodeError):
            return _err(400, "invalid_body", "Corpo JSON inválido.")
        if not isinstance(body, dict):
            return _err(400, "invalid_body", "Corpo JSON inválido.")
        filename = body.get("filename")
        size = body.get("size")
        overwrite = body.get("overwrite", False)
        if not valid_filename(filename):
            return _err(400, "invalid_filename", "Nome de arquivo inválido.")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
            return _err(400, "invalid_size", "size deve ser inteiro positivo.")
        if not isinstance(overwrite, bool):
            return _err(400, "invalid_body", "overwrite deve ser booleano.")

        base = dest_dir()
        final = base / filename
        if final.exists() and not overwrite:
            return _err(409, "file_exists", "O arquivo já existe no destino.")
        uploads_dir = base / UPLOADS_DIR
        await asyncio.to_thread(_prepare_uploads_dir, uploads_dir)
        upload_id = str(uuid.uuid4())
        part = uploads_dir / f"{upload_id}.part"
        await asyncio.to_thread(part.touch)
        uploads[upload_id] = _Upload(filename, size, overwrite, part, final)
        return web.json_response({"uploadId": upload_id, "chunkSize": CHUNK_SIZE}, status=201)

    @routes.put("/hephaestus/lora/uploads/{upload_id}")
    @authorized
    async def put_chunk(request: web.Request) -> web.StreamResponse:
        _, up = lookup(request)
        if up is None:
            return _not_found()
        try:
            offset = int(request.query["offset"])
            if offset < 0:
                raise ValueError
        except (KeyError, ValueError):
            return _err(400, "invalid_offset", "offset ausente ou inválido.")
        declared = request.content_length
        if declared is not None and declared > CHUNK_SIZE:
            return _err(413, "chunk_too_large", f"Parte maior que {CHUNK_SIZE} bytes.")

        async with up.lock:
            current = await asyncio.to_thread(lambda: up.part.stat().st_size)
            if offset != current:
                return _err(409, "offset_mismatch", "Offset diferente do recebido.", expectedOffset=current)
            received = 0
            try:
                async for block in request.content.iter_chunked(_READ_BLOCK):
                    received += len(block)
                    if received > CHUNK_SIZE:
                        await asyncio.to_thread(_truncate, up.part, current)
                        return _err(413, "chunk_too_large", f"Parte maior que {CHUNK_SIZE} bytes.")
                    if current + received > up.size:
                        await asyncio.to_thread(_truncate, up.part, current)
                        return _err(400, "size_exceeded", "Dados passam do tamanho declarado.")
                    await asyncio.to_thread(_append, up.part, block)
            except BaseException:
                # conexão caiu / cancelamento: volta ao estado anterior para retomar
                await asyncio.shield(asyncio.to_thread(_truncate, up.part, current))
                raise
            return web.json_response({"received": current + received})

    @routes.post("/hephaestus/lora/uploads/{upload_id}/commit")
    @authorized
    async def commit(request: web.Request) -> web.StreamResponse:
        uid, up = lookup(request)
        if up is None:
            return _not_found()
        try:
            body = await request.json()
            expected = body["sha256"]
            if not isinstance(expected, str):
                raise TypeError
        except (json.JSONDecodeError, UnicodeDecodeError, KeyError, TypeError):
            return _err(400, "invalid_body", "Informe sha256.")

        async with up.lock:
            actual_size = await asyncio.to_thread(lambda: up.part.stat().st_size)
            if actual_size != up.size:
                await _discard(uid, up)
                return _err(422, "size_mismatch", "Tamanho recebido difere do declarado.")
            digest = await asyncio.to_thread(_sha256_file, up.part)
            if not hmac.compare_digest(digest, expected.strip().lower()):
                await _discard(uid, up)
                return _err(422, "checksum_mismatch", "sha256 não confere.")
            if up.final.exists() and not up.overwrite:
                await _discard(uid, up)
                return _err(409, "file_exists", "O arquivo já existe no destino.")
            await asyncio.to_thread(os.replace, up.part, up.final)
            uploads.pop(uid, None)
        return web.json_response({"path": str(up.final.resolve()), "filename": up.filename})

    @routes.delete("/hephaestus/lora/uploads/{upload_id}")
    @authorized
    async def abort(request: web.Request) -> web.StreamResponse:
        uid, up = lookup(request)
        if up is not None:
            await _discard(uid, up)
        return web.Response(status=204)

    async def _discard(uid: Optional[str], up: _Upload) -> None:
        uploads.pop(uid, None)
        await asyncio.to_thread(lambda: up.part.unlink(missing_ok=True))

    return routes


def _prepare_uploads_dir(uploads_dir: Path) -> None:
    uploads_dir.mkdir(parents=True, exist_ok=True)
    _cleanup_stale(uploads_dir, time.time())


def read_token(token_file: Path) -> Optional[str]:
    env = os.environ.get("HEPHAESTUS_COMFY_TOKEN", "").strip()
    if env:
        return env
    try:
        return token_file.read_text(encoding="utf-8").strip() or None
    except OSError:
        return None
