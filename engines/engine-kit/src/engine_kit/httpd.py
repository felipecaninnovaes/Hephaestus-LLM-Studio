"""
Primitivas de servidor HTTP daemon, serialização JSON e encerramento gracioso via sinais.
"""
import json
import os
import signal
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable, Optional


class JSONHandlerMixin:
    """Mixin para BaseHTTPRequestHandler provendo helpers padronizados para APIs JSON."""

    def send_json(self: Any, status: int, payload: Any) -> None:
        """Envia resposta JSON com cabeçalhos padrão."""
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def read_json_body(self: Any) -> Any:
        """Lê e desserializa o corpo JSON da requisição com base no Content-Length."""
        length_str = self.headers.get("Content-Length", "0")
        try:
            length = int(length_str)
        except (ValueError, TypeError):
            length = 0

        if length <= 0:
            return {}

        raw = self.rfile.read(length)
        return json.loads(raw.decode("utf-8"))

    def log_message(self: Any, format: str, *args: Any) -> None:
        """Suprime logs HTTP barulhentos a menos que ENGINE_HTTP_DEBUG=1 esteja ativo."""
        if os.environ.get("ENGINE_HTTP_DEBUG") == "1":
            sys.stderr.write("%s - - [%s] %s\n" % (self.address_string(), self.log_date_time_string(), format % args))


def run_daemon(
    server: ThreadingHTTPServer,
    name: str = "daemon",
    on_shutdown: Optional[Callable[[], None]] = None,
) -> None:
    """Executa o loop do servidor HTTP com interceptação graciosa de SIGTERM e SIGINT."""
    shutdown_done = threading.Event()

    def _signal_handler(signum: int, _frame: Any) -> None:
        sig_name = "SIGTERM" if signum == signal.SIGTERM else "SIGINT"
        print(f"[{name}] Recebido {sig_name}, iniciando encerramento gracioso...", flush=True)

        if on_shutdown is not None:
            try:
                on_shutdown()
            except Exception as e:
                print(f"[{name}] Erro no callback on_shutdown: {e}", file=sys.stderr, flush=True)

        def _do_shutdown() -> None:
            server.shutdown()
            server.server_close()
            shutdown_done.set()

        # Executa em thread separada para não causar deadlock com o signal handler
        threading.Thread(target=_do_shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, _signal_handler)
    signal.signal(signal.SIGINT, _signal_handler)

    print(f"[{name}] Pronto para receber conexões na porta {server.server_port}.", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        shutdown_done.wait(timeout=5.0)
        print(f"[{name}] Servidor encerrado.", flush=True)
