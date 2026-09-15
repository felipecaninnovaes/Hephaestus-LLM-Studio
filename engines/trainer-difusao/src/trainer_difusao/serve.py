"""Servidor HTTP de inferência quente para o engine trainer-difusao (ADR-0023 D1).

ENGINE_MOCK=1  → geração instantânea e determinística (mock).
ENGINE_MOCK=0  → pipeline real Diffusers com aceleração CUDA (mantido entre requests).

Contrato:
  GET  /health    → {"ok": true, "engine": "diffusion", "mock": bool, "loaded_spec": {...} | null, "busy": bool, "vram_used_gb": float | null, "uptime_s": float, "pid": int}
  POST /generate  → {"ok": true, "items": [...], "cancelled": bool} | erro
  POST /shutdown  → {"ok": true} + encerramento graciosamente

ADR-0023 — Fatia G.3: daemon HTTP de inferência quente.
"""

from __future__ import annotations

import json
import os
import signal
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Estado global do daemon
# ---------------------------------------------------------------------------
_START_TIME = time.time()
_PID = os.getpid()
_MOCK = os.environ.get("ENGINE_MOCK", "1") == "1"

# Lock para serialização (1 request por vez)
_gen_lock = threading.Lock()
_busy = False

# Spec do pipeline residente (dict ou None)
_loaded_spec: dict[str, Any] | None = None

# Referência ao server (para shutdown graceful)
_server: ThreadingHTTPServer | None = None


def _spec_key(spec: dict[str, Any] | None) -> tuple | None:
    """Extrai chave de comparação da spec (base_model, custom, arch, quant, distilled)."""
    if spec is None:
        return None
    return (
        spec.get("base_model"),
        spec.get("custom_checkpoint_path"),
        spec.get("arch"),
        spec.get("quantization"),
        spec.get("distilled"),
    )


def _spec_matches(a: dict[str, Any] | None, b: dict[str, Any] | None) -> bool:
    """Compara duas specs por campos relevantes para reload do pipeline."""
    return _spec_key(a) == _spec_key(b)


def _make_spec(params: dict[str, Any]) -> dict[str, Any]:
    """Extrai dict de spec mínima a partir dos params validados."""
    return {
        "base_model": params.get("base_model"),
        "custom_checkpoint_path": params.get("custom_checkpoint_path"),
        "arch": params.get("arch"),
        "quantization": params.get("quantization"),
        "distilled": params.get("distilled", False),
    }


# ---------------------------------------------------------------------------
# Handler HTTP
# ---------------------------------------------------------------------------
class DiffusionHandler(BaseHTTPRequestHandler):
    """Handler para os endpoints /health, /generate, /shutdown."""

    def log_message(self, *args):
        """Suprime logs padrão do HTTP server (usamos print com flush)."""

    def _send(self, code: int, obj: dict[str, Any]) -> None:
        """Serializa dict para JSON e envia como resposta HTTP."""
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self) -> dict[str, Any] | None:
        """Lê e parseia o body JSON do request."""
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw = self.rfile.read(length) if length > 0 else b""
        if not raw:
            return None
        try:
            return json.loads(raw.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError) as e:
            self._send(400, {"ok": False, "error": "invalid_json", "message": str(e)})
            return None

    # --- GET /health ---
    def do_GET(self):
        if self.path == "/health":
            uptime = round(time.time() - _START_TIME, 1)
            resp = {
                "ok": True,
                "engine": "diffusion",
                "mock": _MOCK,
                "loaded_spec": _loaded_spec,
                "busy": _busy,
                "vram_used_gb": None,
                "uptime_s": uptime,
                "pid": _PID,
            }
            # Tenta capturar VRAM se torch disponível
            if not _MOCK:
                try:
                    import torch

                    if torch.cuda.is_available():
                        resp["vram_used_gb"] = round(
                            torch.cuda.memory_allocated() / (1023**3), 2
                        )
                except (ImportError, RuntimeError):
                    pass
            self._send(200, resp)
        else:
            self._send(404, {"ok": False, "error": "not_found"})

    # --- POST /generate, /shutdown ---
    def do_POST(self):
        if self.path == "/generate":
            self._handle_generate()
        elif self.path == "/shutdown":
            self._handle_shutdown()
        else:
            self._send(404, {"ok": False, "error": "not_found"})

    def _handle_generate(self):
        global _busy, _loaded_spec

        body = self._read_body()
        if body is None:
            return

        # Validação básica do corpo
        if not isinstance(body, dict):
            self._send(
                400,
                {
                    "ok": False,
                    "error": "invalid_request",
                    "message": "Body deve ser um dicionário.",
                },
            )
            return

        config = body.get("config")
        if not isinstance(config, dict):
            self._send(
                400,
                {
                    "ok": False,
                    "error": "invalid_request",
                    "message": "Campo 'config' é obrigatório e deve ser um dicionário.",
                },
            )
            return

        output_dir_str = body.get("output_dir")
        if not output_dir_str or not isinstance(output_dir_str, str):
            self._send(
                400,
                {
                    "ok": False,
                    "error": "invalid_request",
                    "message": "Campo 'output_dir' é obrigatório (string).",
                },
            )
            return

        telemetry_path_str = body.get("telemetry_path")

        # Validação da config via load_and_validate_generate_config
        # Envolve em dict com job_id se não presente (compatibilidade com G.2)
        cfg_for_validate = dict(config)
        if "job_id" not in cfg_for_validate:
            cfg_for_validate["job_id"] = f"daemon-{int(time.time())}"

        try:
            from trainer_difusao.generate import load_and_validate_generate_config

            params = load_and_validate_generate_config(cfg_for_validate)
        except SystemExit as e:
            # load_and_validate_generate_config chama _die() que faz sys.exit
            msg = str(e) if str(e) else "Erro de validação da configuração"
            self._send(
                400,
                {"ok": False, "error": "invalid_config", "message": msg},
            )
            return
        except (ValueError, KeyError, TypeError) as e:
            self._send(
                500,
                {
                    "ok": False,
                    "error": "generation_failed",
                    "message": f"Erro inesperado na validação: {e}",
                },
            )
            return

        # Concorrência: 1 request por vez
        if not _gen_lock.acquire(blocking=False):
            self._send(
                409,
                {
                    "ok": False,
                    "error": "daemon_busy",
                    "message": "Daemon está processando outra requisição. Tente novamente.",
                },
            )
            return

        global _busy
        try:
            _busy = True
            output_dir = Path(output_dir_str)

            # Determina caminho de telemetria
            telemetry_path = None
            if telemetry_path_str:
                telemetry_path = Path(telemetry_path_str)

            # Spec-aware reload
            new_spec = _make_spec(params)
            need_reload = not _spec_matches(_loaded_spec, new_spec)

            if need_reload:
                print(
                    f"[DAEMON] Spec diferente detectada — reload do pipeline. "
                    f"Antes: {_loaded_spec}, Agora: {new_spec}",
                    flush=True,
                )
                # Emite fase loading_model na telemetria
                if telemetry_path:
                    try:
                        from trainer_difusao.telemetry import TelemetryEmitter

                        emitter = TelemetryEmitter(
                            telemetry_path.parent, filename=telemetry_path.name
                        )
                        emitter.emit(
                            phase="loading_model",
                            message=f"Recarregando pipeline para spec: {new_spec}",
                            progress=0.1,
                        )
                    except (ImportError, OSError) as exc:
                        print(
                            f"[DAEMON] Falha ao emitir telemetria de loading: {exc}",
                            flush=True,
                        )
                _loaded_spec = new_spec
            else:
                print("[DAEMON] Spec igual — hot path, sem reload.", flush=True)

            # Executa geração
            print(
                f"[DAEMON] Iniciando geração: mock={_MOCK}, output={output_dir}",
                flush=True,
            )

            from trainer_difusao.generate import _mock_generate, _real_generate

            # Garante que output_dir existe
            output_dir.mkdir(parents=True, exist_ok=True)

            # Se telemetry_path informado, cria emitter customizado para _mock_generate
            # _mock_generate aceita 'emitter' como kwarg ou cria o seu próprio
            if telemetry_path and _MOCK:
                try:
                    from trainer_difusao.telemetry import TelemetryEmitter

                    emitter = TelemetryEmitter(
                        telemetry_path.parent, filename=telemetry_path.name
                    )
                    _mock_generate(params, output_dir, emitter=emitter)
                except TypeError:
                    # Fallback: _mock_generate não aceita emitter kwarg
                    _mock_generate(params, output_dir)
            elif _MOCK:
                _mock_generate(params, output_dir)
            else:
                _real_generate(params, output_dir)

            # Lê itens do generation_meta.json
            items = []
            meta_path = output_dir / "generation_meta.json"
            cancelled = False

            if meta_path.exists():
                lines = [l for l in meta_path.read_text().splitlines() if l.strip()]
                items = [json.loads(l) for l in lines]

            # Verifica cancel
            if (output_dir / "cancel").exists():
                cancelled = True

            self._send(
                200,
                {
                    "ok": True,
                    "items": items,
                    "cancelled": cancelled,
                },
            )

        except (OSError, RuntimeError, ValueError) as e:
            print(f"[DAEMON] Erro na geração: {e}", flush=True)
            self._send(
                500,
                {
                    "ok": False,
                    "error": "generation_failed",
                    "message": str(e),
                },
            )
        finally:
            _busy = False
            _gen_lock.release()

    def _handle_shutdown(self):
        """Encerra o servidor graciosamente."""
        print("[DAEMON] Shutdown solicitado.", flush=True)
        self._send(200, {"ok": True})
        # Agenda shutdown em thread separada para allow response ser enviada
        threading.Thread(target=_do_shutdown_graceful, daemon=True).start()


# ---------------------------------------------------------------------------
# Shutdown helper
# ---------------------------------------------------------------------------
def _do_shutdown_graceful():
    """Executa o shutdown real do servidor via server.shutdown()."""
    time.sleep(0.2)  # Garante que a resposta HTTP foi enviada
    print("[DAEMON] Encerrando servidor...", flush=True)
    if _server is not None:
        _server.shutdown()
    else:
        # Fallback: se referência não disponível
        os._exit(0)


# ---------------------------------------------------------------------------
# Signal handler (SIGTERM)
# ---------------------------------------------------------------------------
def _sigterm_handler(signum, frame):
    """Handler para SIGTERM — mesma rotina do /shutdown."""
    print(f"[DAEMON] Sinal {signum} recebido — shutdown graciosamente.", flush=True)
    _do_shutdown_graceful()


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------
def cmd_serve(args: list[str]) -> None:
    """Subcomando serve: sobe o daemon HTTP de inferência quente."""
    global _server

    import argparse

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

    # Registra handler de SIGTERM (apenas na main thread)
    if threading.current_thread() is threading.main_thread():
        signal.signal(signal.SIGTERM, _sigterm_handler)

    print(
        f"[DAEMON] Iniciando daemon de inferência quente (mock={_MOCK}) na porta {opts.port}...",
        flush=True,
    )

    server = ThreadingHTTPServer(("0.0.0.0", opts.port), DiffusionHandler)
    _server = server

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        _server = None
        print("[DAEMON] Servidor encerrado.", flush=True)


if __name__ == "__main__":
    cmd_serve(sys.argv[1:])
