import json
import threading
import time
import unittest
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from engine_kit.httpd import JSONHandlerMixin


class DummyHandler(JSONHandlerMixin, BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.send_json(200, {"status": "ok", "service": "dummy"})
        else:
            self.send_json(404, {"error": "not_found"})

    def do_POST(self):
        body = self.read_json_body()
        self.send_json(200, {"echo": body})


class TestHttpd(unittest.TestCase):
    def test_json_handler_mixin(self):
        server = ThreadingHTTPServer(("127.0.0.1", 0), DummyHandler)
        port = server.server_port

        t = threading.Thread(target=server.serve_forever, daemon=True)
        t.start()

        time.sleep(0.05)
        try:
            # Test GET /health
            req = urllib.request.Request(f"http://127.0.0.1:{port}/health")
            with urllib.request.urlopen(req) as resp:
                self.assertEqual(resp.status, 200)
                data = json.loads(resp.read().decode("utf-8"))
                self.assertEqual(data["status"], "ok")

            # Test POST with JSON body
            post_data = json.dumps({"foo": "bar"}).encode("utf-8")
            req = urllib.request.Request(
                f"http://127.0.0.1:{port}/echo",
                data=post_data,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req) as resp:
                self.assertEqual(resp.status, 200)
                data = json.loads(resp.read().decode("utf-8"))
                self.assertEqual(data["echo"]["foo"], "bar")
        finally:
            server.shutdown()
            server.server_close()
            t.join(timeout=2.0)
