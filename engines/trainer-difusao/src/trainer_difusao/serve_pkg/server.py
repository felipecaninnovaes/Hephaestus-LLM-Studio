"""
Bootstrap, gerenciamento de ciclo de vida e comandos CLI para o daemon de difusão.
"""
from __future__ import annotations

import argparse
import os
import signal
import sys
import threading
import time
from http.server import ThreadingHTTPServer

import trainer_difusao.serve_pkg.state as state
from trainer_difusao.serve_pkg.handler import DiffusionHandler


def _do_shutdown_graceful() -> None:
    """Executa o shutdown real do servidor via server.shutdown()."""
    time.sleep(0.2)
    print("[DAEMON] Encerrando servidor...", flush=True)
    if state._server is not None:
        state._server.shutdown()
    else:
        os._exit(0)


def _sigterm_handler(signum: int, frame: Any) -> None:
    """Handler para SIGTERM — dispara shutdown em thread para evitar deadlock na thread principal."""
    print(f"[DAEMON] Sinal {signum} recebido — shutdown gracioso.", flush=True)
    threading.Thread(target=_do_shutdown_graceful, daemon=True).start()


def cmd_serve(args: list[str]) -> None:
    """Subcomando serve: sobe o daemon HTTP de inferência quente."""
    parser = argparse.ArgumentParser(
        prog="trainer-difusao serve",
        description="Daemon HTTP de inferência quente — Difusão (mock/real)",
    )
    parser.add_argument(
        "--port",
        type=int,
        required=True,
        help="Porta TCP para o servidor HTTP (1024..65535)",
    )

    opts = parser.parse_args(args)

    if opts.port < 1024 or opts.port > 65535:
        print(
            f"ERROR: Porta inválida: {opts.port}. Deve estar entre 1024 e 65535.",
            file=sys.stderr,
        )
        sys.exit(1)

    if threading.current_thread() is threading.main_thread():
        signal.signal(signal.SIGTERM, _sigterm_handler)

    print(
        f"[DAEMON] Iniciando daemon de inferência quente (mock={state._MOCK}) na porta {opts.port}...",
        flush=True,
    )

    server = ThreadingHTTPServer(("0.0.0.0", opts.port), DiffusionHandler)
    state._server = server

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        state._server = None
        print("[DAEMON] Servidor encerrado.", flush=True)
