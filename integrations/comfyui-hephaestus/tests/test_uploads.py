import hashlib
import os
import time

import pytest
from aiohttp import web

import hephaestus_routes as hr

TOKEN = "segredo"
AUTH = {"Authorization": f"Bearer {TOKEN}"}


def make_app(lora_dir, token=TOKEN):
    app = web.Application()
    app.add_routes(hr.build_routes(lambda: token, lambda: lora_dir))
    return app


@pytest.fixture
def lora_dir(tmp_path):
    return tmp_path / "loras"


@pytest.fixture
async def client(aiohttp_client, lora_dir):
    return await aiohttp_client(make_app(lora_dir))


async def init(client, filename="a.safetensors", size=10, **kw):
    return await client.post(
        "/hephaestus/lora/uploads", json={"filename": filename, "size": size, **kw}, headers=AUTH
    )


async def test_token_not_configured_is_503(aiohttp_client, lora_dir):
    c = await aiohttp_client(make_app(lora_dir, token=None))
    r = await c.get("/hephaestus/health", headers=AUTH)
    assert r.status == 503
    assert (await r.json())["error"] == "token_not_configured"


async def test_wrong_or_missing_token_is_401(client):
    assert (await client.get("/hephaestus/health")).status == 401
    r = await client.get("/hephaestus/health", headers={"Authorization": "Bearer errado"})
    assert r.status == 401


async def test_health(client, lora_dir):
    r = await client.get("/hephaestus/health", headers=AUTH)
    assert r.status == 200
    body = await r.json()
    assert body["version"] == "1" and body["chunkSize"] == 32 * 1024 * 1024
    assert body["loraDir"] == str(lora_dir.resolve())


@pytest.mark.parametrize(
    "name",
    ["../x.safetensors", "a..b.safetensors", "a/b.safetensors", "a.bin", "a b.safetensors",
     "é.safetensors", "a" * 200 + ".safetensors", ""],
)
async def test_invalid_filename_400(client, name):
    r = await init(client, filename=name)
    assert r.status == 400
    assert (await r.json())["error"] == "invalid_filename"


async def test_full_flow_three_parts(client, lora_dir):
    data = os.urandom(3000)
    r = await init(client, size=len(data))
    assert r.status == 201
    uid = (await r.json())["uploadId"]
    for off, end in [(0, 1000), (1000, 2000), (2000, 3000)]:
        r = await client.put(
            f"/hephaestus/lora/uploads/{uid}?offset={off}", data=data[off:end], headers=AUTH
        )
        assert r.status == 200
        assert (await r.json())["received"] == end
    r = await client.post(
        f"/hephaestus/lora/uploads/{uid}/commit",
        json={"sha256": hashlib.sha256(data).hexdigest()},
        headers=AUTH,
    )
    assert r.status == 200
    body = await r.json()
    final = lora_dir / "hephaestus" / "a.safetensors"
    assert body == {"path": str(final.resolve()), "filename": "a.safetensors"}
    assert final.read_bytes() == data
    assert not list((lora_dir / "hephaestus" / ".uploads").glob("*.part"))


async def test_offset_mismatch_and_resume(client, lora_dir):
    data = os.urandom(300)
    uid = (await (await init(client, size=300)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    assert (await client.put(f"{url}?offset=0", data=data[:100], headers=AUTH)).status == 200
    r = await client.put(f"{url}?offset=200", data=data[200:], headers=AUTH)
    assert r.status == 409
    body = await r.json()
    assert body["error"] == "offset_mismatch" and body["expectedOffset"] == 100
    assert (await client.put(f"{url}?offset=100", data=data[100:], headers=AUTH)).status == 200
    r = await client.post(f"{url}/commit", json={"sha256": hashlib.sha256(data).hexdigest()}, headers=AUTH)
    assert r.status == 200
    assert (lora_dir / "hephaestus" / "a.safetensors").read_bytes() == data


async def test_wrong_sha_422_and_part_removed(client, lora_dir):
    uid = (await (await init(client, size=5)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    await client.put(f"{url}?offset=0", data=b"hello", headers=AUTH)
    r = await client.post(f"{url}/commit", json={"sha256": "0" * 64}, headers=AUTH)
    assert r.status == 422
    assert (await r.json())["error"] == "checksum_mismatch"
    assert not list((lora_dir / "hephaestus" / ".uploads").glob("*.part"))
    assert not (lora_dir / "hephaestus" / "a.safetensors").exists()


async def test_size_mismatch_422(client, lora_dir):
    uid = (await (await init(client, size=10)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    await client.put(f"{url}?offset=0", data=b"hello", headers=AUTH)
    r = await client.post(f"{url}/commit", json={"sha256": hashlib.sha256(b"hello").hexdigest()}, headers=AUTH)
    assert r.status == 422
    assert (await r.json())["error"] == "size_mismatch"
    assert not list((lora_dir / "hephaestus" / ".uploads").glob("*.part"))


async def test_put_beyond_declared_size_400_and_state_kept(client):
    uid = (await (await init(client, size=5)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    r = await client.put(f"{url}?offset=0", data=b"123456", headers=AUTH)
    assert r.status == 400
    r = await client.put(f"{url}?offset=0", data=b"12345", headers=AUTH)
    assert r.status == 200


async def test_file_exists_and_overwrite(client, lora_dir):
    final = lora_dir / "hephaestus" / "a.safetensors"
    final.parent.mkdir(parents=True)
    final.write_bytes(b"old")
    r = await init(client)
    assert r.status == 409
    assert (await r.json())["error"] == "file_exists"

    uid = (await (await init(client, size=3, overwrite=True)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    await client.put(f"{url}?offset=0", data=b"new", headers=AUTH)
    r = await client.post(f"{url}/commit", json={"sha256": hashlib.sha256(b"new").hexdigest()}, headers=AUTH)
    assert r.status == 200
    assert final.read_bytes() == b"new"


async def test_put_above_chunk_size_refused(client, lora_dir, monkeypatch):
    monkeypatch.setattr(hr, "CHUNK_SIZE", 100)
    uid = (await (await init(client, size=1000)).json())["uploadId"]
    url = f"/hephaestus/lora/uploads/{uid}"
    r = await client.put(f"{url}?offset=0", data=b"x" * 101, headers=AUTH)
    assert r.status == 413
    # chunked sem Content-Length também é recusado e nada fica gravado
    async def gen():
        for _ in range(3):
            yield b"x" * 50
    r = await client.put(f"{url}?offset=0", data=gen(), headers=AUTH)
    assert r.status == 413
    r = await client.put(f"{url}?offset=0", data=b"x" * 100, headers=AUTH)
    assert r.status == 200


async def test_abort_deletes_part(client, lora_dir):
    uid = (await (await init(client)).json())["uploadId"]
    r = await client.delete(f"/hephaestus/lora/uploads/{uid}", headers=AUTH)
    assert r.status == 204
    assert not list((lora_dir / "hephaestus" / ".uploads").glob("*.part"))
    r = await client.put(f"/hephaestus/lora/uploads/{uid}?offset=0", data=b"x", headers=AUTH)
    assert r.status == 404


async def test_stale_uploads_removed_on_init(client, lora_dir):
    uploads = lora_dir / "hephaestus" / ".uploads"
    uploads.mkdir(parents=True)
    old, fresh = uploads / "old.part", uploads / "fresh.part"
    old.write_bytes(b"x")
    fresh.write_bytes(b"x")
    past = time.time() - 2 * 3600
    os.utime(old, (past, past))
    assert (await init(client)).status == 201
    assert not old.exists() and fresh.exists()


async def test_stale_upload_entry_dropped_then_put_and_commit_404(client, lora_dir):
    uid = (await (await init(client)).json())["uploadId"]
    part = lora_dir / "hephaestus" / ".uploads" / f"{uid}.part"
    past = time.time() - 2 * 3600
    os.utime(part, (past, past))
    assert (await init(client, filename="b.safetensors")).status == 201
    assert not part.exists()
    r = await client.put(f"/hephaestus/lora/uploads/{uid}?offset=0", data=b"x", headers=AUTH)
    assert r.status == 404 and (await r.json())["error"] == "upload_not_found"
    r = await client.post(f"/hephaestus/lora/uploads/{uid}/commit", json={"sha256": "0" * 64}, headers=AUTH)
    assert r.status == 404
    assert not part.exists()


async def test_part_vanished_put_and_commit_404_not_500(client, lora_dir):
    uid = (await (await init(client)).json())["uploadId"]
    (lora_dir / "hephaestus" / ".uploads" / f"{uid}.part").unlink()
    r = await client.put(f"/hephaestus/lora/uploads/{uid}?offset=0", data=b"x", headers=AUTH)
    assert r.status == 404 and (await r.json())["error"] == "upload_not_found"
    uid2 = (await (await init(client, filename="c.safetensors")).json())["uploadId"]
    (lora_dir / "hephaestus" / ".uploads" / f"{uid2}.part").unlink()
    r = await client.post(f"/hephaestus/lora/uploads/{uid2}/commit", json={"sha256": "0" * 64}, headers=AUTH)
    assert r.status == 404


async def test_append_never_recreates_removed_part(tmp_path):
    part = tmp_path / "gone.part"
    with pytest.raises(FileNotFoundError):
        hr._append(part, b"x")
    assert not part.exists()


async def test_size_too_large_400(client):
    r = await init(client, size=hr.MAX_UPLOAD_SIZE + 1)
    assert r.status == 400 and (await r.json())["error"] == "size_too_large"
    monkey_ok = await init(client, size=hr.MAX_UPLOAD_SIZE)
    assert monkey_ok.status in (201, 507)


async def test_insufficient_storage_507(client, lora_dir, monkeypatch):
    class U:
        free = hr.MIN_FREE_BYTES + 99

    monkeypatch.setattr(hr.shutil, "disk_usage", lambda p: U)
    r = await init(client, size=100)
    assert r.status == 507 and (await r.json())["error"] == "insufficient_storage"
    assert not list((lora_dir / "hephaestus" / ".uploads").glob("*.part"))
    assert (await init(client, size=99)).status == 201


async def test_too_many_active_uploads_429_and_slot_freed_by_abort(client):
    uids = []
    for i in range(hr.MAX_ACTIVE_UPLOADS):
        r = await init(client, filename=f"f{i}.safetensors")
        assert r.status == 201
        uids.append((await r.json())["uploadId"])
    r = await init(client, filename="extra.safetensors")
    assert r.status == 429 and (await r.json())["error"] == "too_many_uploads"
    await client.delete(f"/hephaestus/lora/uploads/{uids[0]}", headers=AUTH)
    assert (await init(client, filename="extra.safetensors")).status == 201
