"""Tests for trainer_yolo.autolabel (ADR-0016 D1/D3, passo AL.2).

Covers:
  (a) Config parsing — valid shape, missing keys.
  (b) Mock run in tempdir — captions.jsonl shape D1 ({"filename": "...", "caption": "..."}), metrics.jsonl.
  (c) Determinismo — mesmo seed/filename/prompt gera byte-identical captions.jsonl.
  (d) Inclusão de prompt — prompt customizado é respeitado nas legendas.
  (e) dataset_path vazio/inexistente → erro fail-fast.
  (f) CLI via python -m trainer_yolo autolabel --config ... --output ...
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
import yaml
from trainer_yolo.autolabel import (
    _mock_autolabel,
    _read_dataset_images,
    load_and_validate_autolabel_config,
)


def _make_autolabel_dataset(tmp_path: Path, filenames: list[str]) -> Path:
    ds_dir = tmp_path / "dataset"
    images_dir = ds_dir / "images"
    images_dir.mkdir(parents=True, exist_ok=True)
    for fname in filenames:
        (images_dir / fname).write_bytes(b"\xff\xd8\xff\xe0" + b"\x00" * 10)
    return ds_dir


def _make_config(
    tmp_path: Path,
    dataset_path: Path,
    output_path: Path,
    *,
    seed: int = 42,
    prompt: str | None = None,
) -> Path:
    cfg = {
        "job_id": "job-al-001",
        "engine": "autolabel",
        "model": "mock",
        "mode": "autolabel",
        "dataset_path": str(dataset_path),
        "output_path": str(output_path),
        "seed": seed,
    }
    if prompt is not None:
        cfg["autolabel"] = {"prompt": prompt}
    tmp_path.mkdir(parents=True, exist_ok=True)
    config_path = tmp_path / "config.yaml"
    with open(config_path, "w", encoding="utf-8") as f:
        yaml.safe_dump(cfg, f)
    return config_path


def test_config_validation_success(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img1.jpg"])
    out = tmp_path / "out"
    cfg_path = _make_config(tmp_path, ds, out)
    cfg = load_and_validate_autolabel_config(cfg_path)
    assert cfg["engine"] == "autolabel"
    assert cfg["mode"] == "autolabel"


def test_config_validation_missing_key(tmp_path: Path):
    cfg_path = tmp_path / "bad_config.yaml"
    with open(cfg_path, "w", encoding="utf-8") as f:
        yaml.safe_dump({"job_id": "123"}, f)
    with pytest.raises(SystemExit):
        load_and_validate_autolabel_config(cfg_path)


def test_read_dataset_images(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["b.jpg", "a.png"])
    images = _read_dataset_images(ds)
    assert images == ["a.png", "b.jpg"]


def test_mock_autolabel_outputs(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg"])
    out = tmp_path / "output"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Fotografia macro")
    cfg = load_and_validate_autolabel_config(cfg_path)

    _mock_autolabel(cfg, out)

    captions_file = out / "captions.jsonl"
    assert captions_file.is_file()

    lines = captions_file.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 2

    items = [json.loads(line) for line in lines]
    assert items[0]["filename"] == "img_01.jpg"
    assert "Fotografia macro" in items[0]["caption"]
    assert items[1]["filename"] == "img_02.jpg"
    assert "Fotografia macro" in items[1]["caption"]

    metrics_file = out / "metrics.jsonl"
    assert metrics_file.is_file()


def test_mock_autolabel_determinism(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg"])
    out1 = tmp_path / "out1"
    out2 = tmp_path / "out2"
    cfg1 = load_and_validate_autolabel_config(
        _make_config(tmp_path / "c1", ds, out1, seed=123)
    )
    cfg2 = load_and_validate_autolabel_config(
        _make_config(tmp_path / "c2", ds, out2, seed=123)
    )

    _mock_autolabel(cfg1, out1)
    _mock_autolabel(cfg2, out2)

    content1 = (out1 / "captions.jsonl").read_bytes()
    content2 = (out2 / "captions.jsonl").read_bytes()
    assert content1 == content2


def test_cli_subcommand_autolabel(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["img.jpg"])
    out = tmp_path / "cli_out"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Teste CLI")

    res = subprocess.run(
        [
            sys.executable,
            "-m",
            "trainer_yolo",
            "autolabel",
            "--config",
            str(cfg_path),
            "--output",
            str(out),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert res.returncode == 0, f"CLI failed: {res.stderr}"
    assert (out / "captions.jsonl").is_file()


def test_autolabel_florence_and_qwen_models(tmp_path: Path):
    ds = _make_autolabel_dataset(tmp_path, ["sample.webp"])

    # Florence-2
    out_florence = tmp_path / "florence_out"
    cfg_florence = {
        "job_id": "job-florence",
        "engine": "autolabel",
        "model": "florence-2",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out_florence),
        "seed": 42,
        "autolabel": {"prompt": "Fotografia botânica"},
    }
    cfg_f_path = tmp_path / "florence.yaml"
    cfg_f_path.write_text(yaml.safe_dump(cfg_florence), encoding="utf-8")
    _mock_autolabel(cfg_florence, out_florence)

    lines_f = (
        (out_florence / "captions.jsonl")
        .read_text(encoding="utf-8")
        .strip()
        .splitlines()
    )
    assert len(lines_f) == 1
    data_f = json.loads(lines_f[0])
    assert "Fotografia botânica" in data_f["caption"]
    assert "sharp contours" in data_f["caption"]

    # Qwen2-VL
    out_qwen = tmp_path / "qwen_out"
    cfg_qwen = {
        "job_id": "job-qwen",
        "engine": "autolabel",
        "model": "qwen2-vl",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out_qwen),
        "seed": 42,
        "autolabel": {"prompt": "Inspeção"},
    }
    _mock_autolabel(cfg_qwen, out_qwen)

    lines_q = (
        (out_qwen / "captions.jsonl").read_text(encoding="utf-8").strip().splitlines()
    )
    assert len(lines_q) == 1
    data_q = json.loads(lines_q[0])
    assert "Inspeção" in data_q["caption"]
    assert "high visual definition" in data_q["caption"]


def test_autolabel_openai_fail_fast_on_error(tmp_path: Path):
    """Verifica que erro de conexão com a API OpenAI causa fail-fast sem mascarar o erro."""
    ds = _make_autolabel_dataset(tmp_path, ["test.png"])
    out = tmp_path / "openai_fail_out"
    cfg = {
        "job_id": "job-openai-fail",
        "engine": "autolabel",
        "model": "openai",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out),
        "seed": 42,
        "autolabel": {
            "prompt": "Descreva em detalhes",
            "api_key": "sk-dummy-test-key",
            "api_base": "http://127.0.0.1:9",  # Porta inacessível
            "openai_model": "gpt-4o-mini",
        },
    }
    with pytest.raises(SystemExit) as exc_info:
        _mock_autolabel(cfg, out)
    assert exc_info.value.code == 1


def test_autolabel_openai_mock_server_success(tmp_path: Path):
    """Verifica chamada bem-sucedida à API compatível com OpenAI usando servidor HTTP mock local."""
    import http.server
    import threading

    received_requests = []

    class MockOpenAIHandler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length)
            received_requests.append(json.loads(body.decode("utf-8")))

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            response_payload = {
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "Placa de circuito com solda nítida gerada pela API real.",
                        }
                    }
                ]
            }
            self.wfile.write(json.dumps(response_payload).encode("utf-8"))

        def log_message(self, format, *args):
            # Silencia logs de requisição no console do pytest
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), MockOpenAIHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    try:
        ds = _make_autolabel_dataset(tmp_path, ["sample.png"])
        out = tmp_path / "openai_success_out"
        cfg = {
            "job_id": "job-openai-success",
            "engine": "autolabel",
            "model": "openai",
            "mode": "autolabel",
            "dataset_path": str(ds),
            "output_path": str(out),
            "seed": 42,
            "autolabel": {
                "prompt": "Inspecione os componentes",
                "api_key": "sk-test-token-123",
                "api_base": f"http://127.0.0.1:{port}/v1",
                "openai_model": "gpt-4o-mini",
            },
        }

        _mock_autolabel(cfg, out)

        captions_file = out / "captions.jsonl"
        assert captions_file.is_file()
        lines = captions_file.read_text(encoding="utf-8").strip().splitlines()
        assert len(lines) == 1
        data = json.loads(lines[0])
        assert data["filename"] == "sample.png"
        assert "gerada pela API real" in data["caption"]

        # Verifica que o payload enviado estava no padrão correto da OpenAI
        assert len(received_requests) == 1
        req = received_requests[0]
        assert req["model"] == "gpt-4o-mini"
        messages = req["messages"]
        assert len(messages) == 1
        contents = messages[0]["content"]
        assert contents[0]["type"] == "text"
        assert contents[0]["text"] == "Inspecione os componentes"
        assert contents[1]["type"] == "image_url"
        assert contents[1]["image_url"]["url"].startswith("data:image/png;base64,")
    finally:
        server.shutdown()
        server.server_close()


def test_autolabel_openai_with_test_mock_fallback_env(tmp_path: Path, monkeypatch):
    """Verifica que com AUTOLABEL_TEST_MOCK_FALLBACK=1 o fallback é ativado em caso de erro."""
    monkeypatch.setenv("AUTOLABEL_TEST_MOCK_FALLBACK", "1")
    ds = _make_autolabel_dataset(tmp_path, ["fallback_img.png"])
    out = tmp_path / "openai_fallback_out"
    cfg = {
        "job_id": "job-openai-fb",
        "engine": "autolabel",
        "model": "openai",
        "mode": "autolabel",
        "dataset_path": str(ds),
        "output_path": str(out),
        "seed": 42,
        "autolabel": {
            "prompt": "Prompt de teste",
            "api_key": "sk-dummy",
            "api_base": "http://127.0.0.1:9",
            "openai_model": "gpt-4o-mini",
        },
    }
    _mock_autolabel(cfg, out)
    captions_file = out / "captions.jsonl"
    assert captions_file.is_file()
    data = json.loads(captions_file.read_text(encoding="utf-8").strip())
    assert "OpenAI fallback" in data["caption"]


def test_normalize_api_base_strips_quotes_and_trailing_slashes():
    from trainer_yolo.autolabel import _normalize_api_base

    assert (
        _normalize_api_base('"https://api.openai.com/v1"')
        == "https://api.openai.com/v1"
    )
    assert (
        _normalize_api_base("'https://api.openai.com/v1/'")
        == "https://api.openai.com/v1"
    )
    assert (
        _normalize_api_base('https://llama.felipecncloud.com/v1"')
        == "https://llama.felipecncloud.com/v1"
    )
    assert (
        _normalize_api_base('  "https://llama.felipecncloud.com/v1/"  ')
        == "https://llama.felipecncloud.com/v1"
    )


def test_autolabel_openai_with_reasoning_effort(tmp_path: Path):
    import http.server
    import threading

    from trainer_yolo.autolabel import _call_openai_vision_api

    ds = _make_autolabel_dataset(tmp_path, ["sample.png"])
    img = ds / "images" / "sample.png"
    received_requests = []

    class MockHandler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            data = json.loads(self.rfile.read(length).decode("utf-8"))
            received_requests.append(data)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(
                json.dumps(
                    {
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "Legenda direta sem CoT",
                                }
                            }
                        ],
                    }
                ).encode("utf-8")
            )

        def log_message(self, *args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), MockHandler)
    port = server.server_address[1]
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    try:
        caption = _call_openai_vision_api(
            image_path=img,
            prompt="Descreva a imagem",
            api_key="token",
            api_base=f"http://127.0.0.1:{port}/v1",
            openai_model="test-model",
            reasoning_effort="none",
        )
        assert caption == "Legenda direta sem CoT"
        assert len(received_requests) == 1
        assert received_requests[0].get("reasoning_effort") == "none"
    finally:
        server.shutdown()
        server.server_close()


def _call_with_mock_choice(tmp_path: Path, choice: dict):
    """Chama _call_openai_vision_api contra um servidor HTTP local que devolve `choice`."""
    import http.server
    import threading

    from trainer_yolo.autolabel import _call_openai_vision_api

    ds = _make_autolabel_dataset(tmp_path, ["sample.png"])
    img = ds / "images" / "sample.png"
    received: list[dict] = []

    class MockHandler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            received.append(json.loads(self.rfile.read(length).decode("utf-8")))
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"choices": [choice]}).encode("utf-8"))

        def log_message(self, *args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), MockHandler)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        caption = _call_openai_vision_api(
            image_path=img,
            prompt="Descreva a imagem",
            api_key="token",
            api_base=f"http://127.0.0.1:{port}/v1",
            openai_model="test-model",
        )
    finally:
        server.shutdown()
        server.server_close()
    return caption, received


def test_autolabel_openai_reasoning_exhausted_budget_never_captions_thinking(
    tmp_path: Path,
):
    choice = {
        "message": {
            "role": "assistant",
            "content": "",
            "reasoning_content": "PENSAMENTO SECRETO",
        },
        "finish_reason": "length",
    }
    with pytest.raises(RuntimeError, match="AUTOLABEL_MAX_TOKENS") as exc_info:
        _call_with_mock_choice(tmp_path, choice)
    assert "limite de tokens" in str(exc_info.value)
    assert "PENSAMENTO SECRETO" not in str(exc_info.value)


def test_autolabel_openai_strips_think_blocks(tmp_path: Path):
    choice = {
        "message": {"role": "assistant", "content": "<think>x</think>  Final caption"},
        "finish_reason": "stop",
    }
    caption, _ = _call_with_mock_choice(tmp_path, choice)
    assert caption == "Final caption"


def test_autolabel_openai_unterminated_think_raises(tmp_path: Path):
    choice = {
        "message": {"role": "assistant", "content": "<think>ainda pensando..."},
        "finish_reason": "stop",
    }
    with pytest.raises(RuntimeError, match="resposta final"):
        _call_with_mock_choice(tmp_path, choice)


def test_autolabel_openai_truncated_caption_raises(tmp_path: Path):
    choice = {
        "message": {"role": "assistant", "content": "Legenda cortada no me"},
        "finish_reason": "length",
    }
    with pytest.raises(RuntimeError, match="truncada"):
        _call_with_mock_choice(tmp_path, choice)


def test_autolabel_openai_max_tokens_only_when_env_set(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    choice = {
        "message": {"role": "assistant", "content": "ok"},
        "finish_reason": "stop",
    }
    monkeypatch.delenv("AUTOLABEL_MAX_TOKENS", raising=False)
    _, received = _call_with_mock_choice(tmp_path / "a", choice)
    assert "max_tokens" not in received[0]
    assert "max_completion_tokens" not in received[0]

    monkeypatch.setenv("AUTOLABEL_MAX_TOKENS", "4096")
    _, received = _call_with_mock_choice(tmp_path / "b", choice)
    assert received[0]["max_tokens"] == 4096

    monkeypatch.setenv("AUTOLABEL_MAX_TOKENS", "-5")
    _, received = _call_with_mock_choice(tmp_path / "c", choice)
    assert "max_tokens" not in received[0]


def test_autolabel_telemetry_progress_and_metrics_contract(tmp_path: Path):
    """Verifica que o autolabel reporta progresso por imagem na telemetria canônica
    (step 1..N, totalSteps, progresso crescente, phase 'labeling') e que nenhuma linha
    contém chaves YOLO fabricadas (box_loss, mAP50, etc.)."""
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg", "img_03.jpg"])
    out = tmp_path / "output_telemetry"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Teste telemetria")
    cfg = load_and_validate_autolabel_config(cfg_path)

    _mock_autolabel(cfg, out)

    telem_file = out / "telemetry.jsonl"
    assert telem_file.is_file(), "telemetry.jsonl deve existir"

    telem_lines = [json.loads(line) for line in telem_file.read_text(encoding="utf-8").strip().splitlines()]
    # 1 preparing + 3 labeling (1 por imagem) + 1 completed = 5 eventos
    assert len(telem_lines) == 5

    # Evento 0: preparing
    assert telem_lines[0]["phase"] == "preparing"
    assert telem_lines[0]["progress"] == 0.05

    # Eventos 1..3: labeling por imagem
    labeling_events = telem_lines[1:4]
    prev_progress = 0.05
    for idx, ev in enumerate(labeling_events, start=1):
        assert ev["phase"] == "labeling"
        assert ev["step"] == idx
        assert ev["totalSteps"] == 3
        assert ev["progress"] > prev_progress
        assert ev["progress"] < 1.0
        assert f"Anotando imagem {idx}/3:" in ev["phaseMessage"]
        assert f"img_0{idx}.jpg" in ev["phaseMessage"]
        prev_progress = ev["progress"]

    # Evento final: completed
    assert telem_lines[4]["phase"] == "completed"
    assert telem_lines[4]["progress"] == 1.0

    # Nenhuma linha em telemetry.jsonl contém métricas YOLO fabricadas
    for ev in telem_lines:
        assert "box_loss" not in ev
        assert "cls_loss" not in ev
        assert "dfl_loss" not in ev
        assert "mAP50" not in ev
        assert "mAP50-95" not in ev
        metrics_dict = ev.get("metrics") or {}
        assert "box_loss" not in metrics_dict
        assert "mAP50" not in metrics_dict

    # Verifica o espelho em metrics.jsonl gerado pelo TelemetryEmitter
    metrics_file = out / "metrics.jsonl"
    assert metrics_file.is_file(), "metrics.jsonl deve ser gerado pelo espelho do emitter"
    metrics_lines = [json.loads(line) for line in metrics_file.read_text(encoding="utf-8").strip().splitlines()]
    assert len(metrics_lines) == 5
    for m in metrics_lines:
        assert "box_loss" not in m
        assert "cls_loss" not in m
        assert "dfl_loss" not in m
        assert "mAP50" not in m
        assert "mAP50-95" not in m


def test_autolabel_telemetry_eta_and_step_time(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Verifica que o autolabel calcula stepTimeSeconds e etaSeconds decrescente até 0 na última imagem."""
    import time

    # Simula passagem de tempo determinística: cada imagem consome 2.0s
    current_time = 1000.0

    def fake_monotonic():
        nonlocal current_time
        current_time += 1.0
        return current_time

    monkeypatch.setattr(time, "monotonic", fake_monotonic)

    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg", "img_03.jpg"])
    out = tmp_path / "output_eta"
    cfg_path = _make_config(tmp_path, ds, out, prompt="Teste ETA")
    cfg = load_and_validate_autolabel_config(cfg_path)

    _mock_autolabel(cfg, out)

    telem_file = out / "telemetry.jsonl"
    assert telem_file.is_file()

    telem_lines = [json.loads(line) for line in telem_file.read_text(encoding="utf-8").strip().splitlines()]
    labeling_events = telem_lines[1:4]

    # Verifica presença de stepTimeSeconds e etaSeconds
    for ev in labeling_events:
        assert "stepTimeSeconds" in ev
        assert ev["stepTimeSeconds"] > 0
        assert "etaSeconds" in ev

    # etaSeconds deve ser decrescente até 0 na última imagem
    assert labeling_events[0]["step"] == 1
    assert labeling_events[0]["etaSeconds"] > labeling_events[1]["etaSeconds"]
    assert labeling_events[1]["step"] == 2
    assert labeling_events[1]["etaSeconds"] > labeling_events[2]["etaSeconds"]
    assert labeling_events[2]["step"] == 3
    assert labeling_events[2]["etaSeconds"] == 0


def test_autolabel_retains_progress_on_midway_die(tmp_path: Path):
    """Verifica que quando o autolabel falha no meio, o último progresso real
    fica registrado na telemetria antes da finalização."""
    import http.server
    import threading

    request_count = 0

    class FlakyOpenAIHandler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            nonlocal request_count
            request_count += 1
            length = int(self.headers.get("Content-Length", 0))
            self.rfile.read(length)
            if request_count == 1:
                # Imagem 1: sucesso
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                res = {"choices": [{"message": {"role": "assistant", "content": "Legenda 1"}}]}
                self.wfile.write(json.dumps(res).encode("utf-8"))
            else:
                # Imagem 2: erro fatal
                self.send_response(500)
                self.end_headers()

        def log_message(self, format, *args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), FlakyOpenAIHandler)
    port = server.server_address[1]
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    try:
        ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg", "img_03.jpg"])
        out = tmp_path / "output_fail"
        cfg = {
            "job_id": "job-openai-mid-fail",
            "engine": "autolabel",
            "model": "openai",
            "mode": "autolabel",
            "dataset_path": str(ds),
            "output_path": str(out),
            "seed": 42,
            "autolabel": {
                "prompt": "Teste",
                "api_key": "sk-dummy-test-key",
                "api_base": f"http://127.0.0.1:{port}/v1",
                "openai_model": "gpt-4o-mini",
            },
        }
        with pytest.raises(SystemExit):
            _mock_autolabel(cfg, out)

        telem_file = out / "telemetry.jsonl"
        assert telem_file.is_file()
        telem_lines = [json.loads(line) for line in telem_file.read_text(encoding="utf-8").strip().splitlines()]
        # Evento 0: preparing
        # Evento 1: labeling img_01 (step=1) antes de falhar na img_02
        assert len(telem_lines) == 2
        assert telem_lines[0]["phase"] == "preparing"
        assert telem_lines[1]["phase"] == "labeling"
        assert telem_lines[1]["step"] == 1
        assert telem_lines[1]["totalSteps"] == 3
        assert "Anotando imagem 1/3: img_01.jpg" in telem_lines[1]["phaseMessage"]
    finally:
        server.shutdown()
        server.server_close()


def test_autolabel_openai_fail_fast_out_of_order(tmp_path: Path):
    """Verifica que quando uma imagem posterior falha rapidamente enquanto uma anterior é lenta,
    o pipeline aborta imediatamente (fail-fast) e nenhuma nova tarefa é submetida."""
    import http.server
    import os
    import socketserver
    import threading
    import time

    submitted_requests = []

    class ThreadedServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
        daemon_threads = True

    class OutOfOrderHandler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            # Identifica a imagem pelo prompt ou ordem
            submitted_requests.append(time.monotonic())
            
            # Se for a 1ª requisição (img_01), faz delay artificial de 0.8s
            # Se for a 2ª requisição (img_02), falha imediatamente com 500 sem retry (ou 400 que não tem retry)
            if len(submitted_requests) == 1:
                time.sleep(0.8)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                res = {"choices": [{"message": {"role": "assistant", "content": "Legenda 1"}}]}
                self.wfile.write(json.dumps(res).encode("utf-8"))
            else:
                # Imagem 2 falha de imediato com 400 Bad Request (sem retry no vision_api)
                self.send_response(400)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"error": {"message": "Erro fatal imediato"}}).encode("utf-8"))

        def log_message(self, *args):
            pass

    server = ThreadedServer(("127.0.0.1", 0), OutOfOrderHandler)
    port = server.server_address[1]
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()

    # Dataset com 5 imagens e concorrência 2:
    # Inicialmente sobem img_01 e img_02. img_02 falha imediatamente.
    # Com o fail-fast imediato, o loop morre sem submeter img_03, img_04, img_05!
    ds = _make_autolabel_dataset(tmp_path, ["img_01.jpg", "img_02.jpg", "img_03.jpg", "img_04.jpg", "img_05.jpg"])
    out = tmp_path / "out_fail_fast"
    cfg = {
        "job_id": "job-fail-fast",
        "model": "openai",
        "dataset_path": str(ds),
        "output_path": str(out),
        "seed": 42,
        "autolabel": {
            "prompt": "Teste",
            "api_key": "token",
            "api_base": f"http://127.0.0.1:{port}/v1",
            "openai_model": "gpt-4o",
        },
    }

    old_conc = os.environ.get("AUTOLABEL_CONCURRENCY")
    os.environ["AUTOLABEL_CONCURRENCY"] = "2"
    try:
        with pytest.raises(SystemExit) as exc_info:
            _mock_autolabel(cfg, out)
        assert exc_info.value.code == 1
        # img_01 e img_02 foram submetidas na janela inicial de tamanho 2.
        # img_03, img_04, img_05 NUNCA devem ter sido submetidas!
        assert len(submitted_requests) == 2, f"Esperado apenas 2 requisições, mas {len(submitted_requests)} foram enviadas!"
    finally:
        if old_conc is not None:
            os.environ["AUTOLABEL_CONCURRENCY"] = old_conc
        else:
            os.environ.pop("AUTOLABEL_CONCURRENCY", None)
        server.shutdown()
        server.server_close()
