"""
Handler HTTP para os endpoints /health, /generate e /shutdown do daemon.
"""
from __future__ import annotations

import json
import threading
import time
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from typing import Any

import yaml
from engine_kit.vram import vram_allocated_gb
import trainer_difusao.serve_pkg.state as state
from trainer_difusao.serve_pkg.state import _make_spec, _spec_matches


class DiffusionHandler(BaseHTTPRequestHandler):
    """Handler para os endpoints /health, /generate, /shutdown."""

    def log_message(self, *args: Any) -> None:
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
            return {}
        try:
            return json.loads(raw.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._send(
                400,
                {"ok": False, "error": "invalid_json", "message": "Body não é JSON válido."},
            )
            return None

    def do_GET(self) -> None:
        if self.path == "/health":
            uptime = round(time.time() - state._START_TIME, 1)
            resp = {
                "ok": True,
                "engine": "diffusion",
                "mock": state._MOCK,
                "loaded_spec": state._loaded_spec,
                "busy": state._busy,
                "vram_used_gb": None,
                "uptime_s": uptime,
                "pid": state._PID,
            }
            if not state._MOCK:
                resp["vram_used_gb"] = vram_allocated_gb()
            self._send(200, resp)
        else:
            self._send(404, {"ok": False, "error": "not_found"})

    def do_POST(self) -> None:
        if self.path == "/generate":
            self._handle_generate()
        elif self.path == "/shutdown":
            self._handle_shutdown()
        else:
            self._send(404, {"ok": False, "error": "not_found"})

    def _handle_generate(self) -> None:
        body = self._read_body()
        if body is None:
            return

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
        if isinstance(config, str):
            try:
                config = yaml.safe_load(config)
            except (yaml.YAMLError, ValueError) as e:
                self._send(
                    400,
                    {
                        "ok": False,
                        "error": "invalid_request",
                        "message": f"Campo 'config' é uma string YAML inválida: {e}",
                    },
                )
                return
            if not isinstance(config, dict):
                self._send(
                    400,
                    {
                        "ok": False,
                        "error": "invalid_request",
                        "message": (
                            "Campo 'config' (string YAML) não parseou para um dicionário."
                        ),
                    },
                )
                return
        elif not isinstance(config, dict):
            self._send(
                400,
                {
                    "ok": False,
                    "error": "invalid_request",
                    "message": "Campo 'config' é obrigatório e deve ser dict ou string YAML.",
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

        cfg_for_validate = dict(config)
        if "job_id" not in cfg_for_validate:
            cfg_for_validate["job_id"] = f"daemon-{int(time.time())}"

        try:
            from trainer_difusao.generate import load_and_validate_generate_config

            params = load_and_validate_generate_config(cfg_for_validate)
        except SystemExit as e:
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

        if not state._gen_lock.acquire(blocking=False):
            self._send(
                409,
                {
                    "ok": False,
                    "error": "daemon_busy",
                    "message": "Daemon está processando outra requisição. Tente novamente.",
                },
            )
            return

        try:
            state._busy = True
            output_dir = Path(output_dir_str)

            telemetry_path = None
            if telemetry_path_str:
                telemetry_path = Path(telemetry_path_str)

            from trainer_difusao.generate import ensure_pipeline

            new_spec = _make_spec(params)
            need_reload = not _spec_matches(state._loaded_spec, new_spec)

            cached_pipeline, cache_key = ensure_pipeline(params, state._pipeline_cache)

            if need_reload:
                print(
                    f"[DAEMON] Spec diferente detectada — reload do pipeline. "
                    f"Antes: {state._loaded_spec}, Agora: {new_spec}",
                    flush=True,
                )
                state._pipeline_cache.clear()
                cached_pipeline = None
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
                state._loaded_spec = new_spec
            else:
                print("[DAEMON] Spec igual — hot path, sem reload.", flush=True)

            print(
                f"[DAEMON] Iniciando geração: mock={state._MOCK}, output={output_dir}",
                flush=True,
            )

            from trainer_difusao.generate import _mock_generate, _real_generate

            output_dir.mkdir(parents=True, exist_ok=True)

            if telemetry_path and state._MOCK:
                try:
                    from trainer_difusao.telemetry import TelemetryEmitter

                    emitter = TelemetryEmitter(
                        telemetry_path.parent, filename=telemetry_path.name
                    )
                    _mock_generate(params, output_dir, emitter=emitter)
                except TypeError:
                    _mock_generate(params, output_dir)
            elif state._MOCK:
                _mock_generate(params, output_dir)
            else:
                loaded_pipe = _real_generate(
                    params, output_dir, pipeline=cached_pipeline
                )
                if loaded_pipe is not None and cache_key is not None:
                    state._pipeline_cache.clear()
                    state._pipeline_cache[cache_key] = loaded_pipe

            items = []
            meta_path = output_dir / "generation_meta.json"
            cancelled = False

            if meta_path.exists():
                lines = [l for l in meta_path.read_text().splitlines() if l.strip()]
                items = [json.loads(l) for l in lines]

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
            state._busy = False
            state._gen_lock.release()

    def _handle_shutdown(self) -> None:
        """Encerra o servidor graciosamente."""
        from trainer_difusao.serve_pkg.server import _do_shutdown_graceful

        print("[DAEMON] Shutdown solicitado.", flush=True)
        self._send(200, {"ok": True})
        threading.Thread(target=_do_shutdown_graceful, daemon=True).start()
