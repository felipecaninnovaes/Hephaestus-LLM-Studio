import base64
import json
import math
import threading
import time
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer

from trainer_clip.mock_embed import mock_vector
from trainer_clip.server import DIM, Handler


class TestTrainerClipServer(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.port = cls.server.server_port
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        time.sleep(0.05)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2.0)

    def _post(self, path: str, data: dict) -> tuple[int, dict]:
        url = f"http://127.0.0.1:{self.port}{path}"
        payload = json.dumps(data).encode("utf-8")
        req = urllib.request.Request(
            url, data=payload, headers={"Content-Type": "application/json"}, method="POST"
        )
        try:
            with urllib.request.urlopen(req, timeout=5.0) as resp:
                body = json.loads(resp.read().decode("utf-8"))
                return resp.status, body
        except urllib.error.HTTPError as e:
            body = json.loads(e.read().decode("utf-8"))
            return e.code, body

    def test_health_endpoint(self):
        url = f"http://127.0.0.1:{self.port}/health"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req, timeout=5.0) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode("utf-8"))
            self.assertEqual(data["status"], "ok")
            self.assertEqual(data["mode"], "mock")
            self.assertEqual(data["dim"], DIM)

    def test_embed_mock(self):
        raw_bytes = b"fake_image_payload_bytes_123"
        b64 = base64.b64encode(raw_bytes).decode("utf-8")

        status, resp = self._post(
            "/embed",
            {"model": "ViT-B-32", "items": [{"id": "img1", "b64": b64}]},
        )
        self.assertEqual(status, 200)
        self.assertIn("items", resp)
        self.assertEqual(len(resp["items"]), 1)
        item = resp["items"][0]
        self.assertEqual(item["id"], "img1")
        self.assertEqual(len(item["vector"]), 512)
        norm = math.sqrt(sum(x * x for x in item["vector"]))
        self.assertAlmostEqual(norm, 1.0, places=3)

    def test_embed_text_mock(self):
        status, resp = self._post(
            "/embed-text",
            {"model": "ViT-B-32", "texts": ["a photo of a circuit board", "electronic components"]},
        )
        self.assertEqual(status, 200)
        self.assertIn("items", resp)
        self.assertEqual(len(resp["items"]), 2)
        self.assertEqual(resp["items"][0]["id"], 0)
        self.assertEqual(resp["items"][1]["id"], 1)
        self.assertEqual(len(resp["items"][0]["vector"]), 512)
        self.assertEqual(len(resp["items"][1]["vector"]), 512)

    def test_embed_bad_shape(self):
        status, resp = self._post("/embed", {"invalid": "shape"})
        self.assertEqual(status, 400)
        self.assertEqual(resp["error"], "bad_shape")

    def test_embed_bad_b64(self):
        status, resp = self._post(
            "/embed",
            {"model": "ViT-B-32", "items": [{"id": "bad", "b64": "not_base_64!!!"}]},
        )
        self.assertEqual(status, 400)
        self.assertIn("bad_b64", resp["error"])
