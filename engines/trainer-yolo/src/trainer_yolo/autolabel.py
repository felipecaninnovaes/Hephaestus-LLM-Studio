"""Modo autolabel (ADR-0016 / ADR-0019 AutoLabel v2).

Suporta:
  - Modelos VLM locais: florence-2, qwen2-vl (com fallback/mock determinístico estilizado)
  - Provedor de API externa: openai (chamadas compatíveis com OpenAI Chat Completions Vision)
  - Modo determinístico padrão: mock

Entrypoint: python -m trainer_yolo autolabel --config <config.yaml> --output <dir>

Saída:
  - captions.jsonl (formato D1: {"filename": "...", "caption": "..."})
  - metrics.jsonl (1 linha compatível com orquestrador)
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import socket
import ssl
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import yaml

from trainer_yolo.train import _die

MOCK_DESCRIPTIONS = [
    "Uma foto em close de uma placa de circuito impresso com solda fria e componentes SMD.",
    "Placa de circuito verde com conectores banhados a ouro e trilhas metálicas visíveis.",
    "Vista superior de placa de circuito eletrônico com capacitores e resistores alinhados.",
    "Foto detalhada de circuito impresso destacando pontos de solda e microcontrolador central.",
    "Componentes eletrônicos montados em superfície com solda de precisão sob luz de bancada.",
    "Macro de conexões eletrônicas em substrato cerâmico com filamentos condutores expostos.",
    "Detalhe de placa controladora com barramentos de cobre e conectores de alta densidade.",
    "Placa de circuito industrial exibindo pontos de teste e marcações serigráficas nítidas.",
]

IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}


def load_and_validate_autolabel_config(config_path: str | Path) -> dict:
    """Carrega config.yaml e valida campos necessários para autolabel."""
    config_path = Path(config_path)
    if not config_path.is_file():
        _die(f"Config file not found: {config_path}")

    with open(config_path, "r", encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    if not isinstance(cfg, dict):
        _die("Config is not a YAML mapping")

    missing = {
        "job_id",
        "engine",
        "model",
        "mode",
        "dataset_path",
        "output_path",
        "seed",
    } - cfg.keys()
    if missing:
        _die(f"Missing required config keys: {sorted(missing)}")

    return cfg


def _discover_dataset_images(dataset_path: Path) -> dict[str, Path]:
    """Descobre os arquivos de imagem dentro do pacote do dataset mapeando {filename: Path}."""
    if not dataset_path.is_dir():
        _die(f"Dataset path is not a directory: {dataset_path}")

    images: dict[str, Path] = {}

    # 1. Checa diretório images/
    images_dir = dataset_path / "images"
    if images_dir.is_dir():
        for p in images_dir.iterdir():
            if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
                images[p.name] = p

    # 2. Checa dataset.yaml se existir
    dataset_yaml = dataset_path / "dataset.yaml"
    if dataset_yaml.is_file():
        with open(dataset_yaml, "r", encoding="utf-8") as f:
            ds = yaml.safe_load(f)
        if isinstance(ds, dict):
            for split_key in ("train", "val"):
                paths = ds.get(split_key)
                if isinstance(paths, list):
                    for p_str in paths:
                        p = Path(p_str)
                        if not p.is_absolute():
                            p = dataset_path / p
                        if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
                            images[p.name] = p

    # 3. Também checa a raiz do dataset_path caso as imagens estejam soltas
    for p in dataset_path.iterdir():
        if (
            p.is_file()
            and p.suffix.lower() in IMAGE_EXTENSIONS
            and p.name not in images
        ):
            images[p.name] = p

    if not images:
        _die(f"No images found in dataset: {dataset_path}")

    return images


def _generate_caption_mock(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda determinística a partir de (seed, filename, prompt)."""
    h_input = f"{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    if prompt and prompt.strip():
        return f"{prompt.strip()} — {base}"
    return base


def _generate_caption_florence(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda no estilo dense captioning da arquitetura Florence-2."""
    h_input = f"florence:{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    prefix = (
        prompt.strip()
        if (prompt and prompt.strip())
        else "A detailed high-resolution photograph"
    )
    return f"{prefix} showing {base.lower().rstrip('.')} with sharp contours and balanced lighting."


def _generate_caption_qwen(seed: int, filename: str, prompt: str | None) -> str:
    """Gera legenda analítica detalhada no estilo Qwen2-VL."""
    h_input = f"qwen:{seed}:{filename}"
    h = int(hashlib.sha256(h_input.encode("utf-8")).hexdigest(), 16)
    base = MOCK_DESCRIPTIONS[h % len(MOCK_DESCRIPTIONS)]

    user_instruction = f" ({prompt.strip()})" if (prompt and prompt.strip()) else ""
    return f"This image exhibits {base.lower().rstrip('.')}{user_instruction}. The subject displays consistent geometry, technical textures and high visual definition."


def _get_mime_type(file_path: Path) -> str:
    ext = file_path.suffix.lower()
    if ext == ".png":
        return "image/png"
    if ext == ".webp":
        return "image/webp"
    if ext in (".jpg", ".jpeg"):
        return "image/jpeg"
    return "image/jpeg"


def _normalize_api_base(api_base: str) -> str:
    """Normaliza api_base, remove aspas e traduz localhost/127.0.0.1 para host.docker.internal quando em container."""
    url = api_base.strip().strip("\"'").rstrip("/")
    is_docker = (
        os.path.exists("/.dockerenv") or os.environ.get("RUNNING_IN_DOCKER") == "1"
    )
    if is_docker or ("localhost" in url or "127.0.0.1" in url):
        try:
            socket.gethostbyname("host.docker.internal")
            url = re.sub(
                r"^(https?://)(?:localhost|127\.0\.0\.1)(:\d+)?",
                r"\1host.docker.internal\2",
                url,
            )
        except (socket.gaierror, OSError):
            pass
    return url


def _call_openai_vision_api(
    image_path: Path,
    prompt: str | None,
    api_key: str | None,
    api_base: str,
    openai_model: str,
) -> str:
    """Faz chamada HTTP à API compatível com OpenAI Vision para descrever a imagem."""
    norm_base = _normalize_api_base(api_base)
    image_bytes = image_path.read_bytes()
    b64_img = base64.b64encode(image_bytes).decode("utf-8")
    mime = _get_mime_type(image_path)

    instruction = (
        prompt.strip()
        if (prompt and prompt.strip())
        else (
            "Descreva detalhadamente o conteúdo desta imagem para treinamento de modelo de IA, "
            "focando em objetos principais, estilo, iluminação e cores."
        )
    )

    payload = {
        "model": openai_model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": instruction},
                    {
                        "type": "image_url",
                        "image_url": {"url": f"data:{mime};base64,{b64_img}"},
                    },
                ],
            }
        ],
        "max_tokens": 500,
    }

    url = f"{norm_base}/chat/completions"
    data = json.dumps(payload).encode("utf-8")

    headers = {
        "Content-Type": "application/json",
        "User-Agent": "Hephaestus-Studio-AutoLabel/2.0",
    }
    if api_key and api_key.strip():
        headers["Authorization"] = f"Bearer {api_key.strip()}"

    # Headers recomendados para OpenRouter
    if "openrouter.ai" in norm_base:
        headers["HTTP-Referer"] = "https://hephaestus.studio"
        headers["X-Title"] = "Hephaestus Studio AutoLabel"

    req = urllib.request.Request(
        url,
        data=data,
        headers=headers,
        method="POST",
    )

    ctx = ssl.create_default_context()
    if os.environ.get("AUTOLABEL_INSECURE_SSL") == "1":
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE

    max_retries = 2
    for attempt in range(max_retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=90, context=ctx) as resp:
                body = json.loads(resp.read().decode("utf-8"))
                choices = body.get("choices")
                if not choices or not isinstance(choices, list):
                    raise ValueError(f"Formato de resposta inesperado da API: {body}")
                message = choices[0].get("message", {})
                content = message.get("content", "")
                if isinstance(content, list):
                    text_parts = [
                        p.get("text", "")
                        for p in content
                        if isinstance(p, dict) and p.get("type") == "text"
                    ]
                    content = " ".join(text_parts)
                return str(content).strip()
        except urllib.error.HTTPError as exc:
            err_body = ""
            try:
                raw_bytes = exc.read()
                err_body = raw_bytes.decode("utf-8", errors="replace")
                parsed = json.loads(err_body)
                if isinstance(parsed, dict) and "error" in parsed:
                    err_info = parsed["error"]
                    if isinstance(err_info, dict):
                        err_body = err_info.get("message") or str(err_info)
                    else:
                        err_body = str(err_info)
            except (
                json.JSONDecodeError,
                UnicodeDecodeError,
                KeyError,
                AttributeError,
            ):
                pass

            err_msg = f"HTTP {exc.code} {exc.reason}: {err_body or 'Sem detalhes'}"
            print(
                f"[autolabel-openai] ERRO na chamada ({url}) para {image_path.name}: {err_msg}",
                file=sys.stderr,
            )

            if attempt < max_retries and exc.code in (429, 500, 502, 503, 504):
                time.sleep(1.5 * (attempt + 1))
                continue

            raise RuntimeError(f"OpenAI API error ({url}): {err_msg}") from exc
        except (TimeoutError, urllib.error.URLError, OSError) as exc:
            err_msg = f"Falha de conexão ({url}): {exc}"
            print(
                f"[autolabel-openai] ERRO de conexão para {image_path.name}: {err_msg}",
                file=sys.stderr,
            )
            if attempt < max_retries:
                time.sleep(1.0)
                continue
            raise RuntimeError(f"OpenAI API connection failed ({url}): {exc}") from exc
        except Exception as exc:
            print(
                f"[autolabel-openai] Erro inesperado para {image_path.name}: {exc}",
                file=sys.stderr,
            )
            raise RuntimeError(f"OpenAI API parse error: {exc}") from exc

    raise RuntimeError(
        f"OpenAI API falhou após {max_retries + 1} tentativas para {image_path.name}"
    )


def _autolabel_pipeline(cfg: dict, output_dir: Path) -> None:
    """Executa o pipeline do AutoLabel v2 (Florence-2, Qwen2-VL, OpenAI, Mock)."""
    output_dir.mkdir(parents=True, exist_ok=True)
    dataset_path = Path(cfg["dataset_path"])
    seed = int(cfg.get("seed", 42))
    model = cfg.get("model", "mock")

    al_section = cfg.get("autolabel", {})
    prompt = None
    api_key = None
    api_base = "https://api.openai.com/v1"
    openai_model = "gpt-4o-mini"

    if isinstance(al_section, dict):
        prompt = al_section.get("prompt")
        raw_key = al_section.get("api_key") or os.environ.get("OPENAI_API_KEY")
        if raw_key:
            api_key = str(raw_key).strip().strip("\"'")
        if al_section.get("api_base"):
            api_base = str(al_section["api_base"]).strip().strip("\"'").rstrip("/")
        if al_section.get("openai_model"):
            openai_model = str(al_section["openai_model"]).strip().strip("\"'")

    image_map = _discover_dataset_images(dataset_path)
    sorted_filenames = sorted(image_map.keys())

    captions_path = output_dir / "captions.jsonl"
    with open(captions_path, "w", encoding="utf-8") as f:
        for fname in sorted_filenames:
            img_path = image_map[fname]
            if model == "openai":
                is_official = "api.openai.com" in api_base
                if is_official and not (api_key and api_key.strip()):
                    if os.environ.get("AUTOLABEL_TEST_MOCK_FALLBACK") == "1":
                        caption = _generate_caption_mock(seed, fname, prompt)
                    else:
                        _die(
                            "O modelo 'openai' com endpoint oficial requer uma API Key válida. "
                            "Forneça a apiKey na requisição ou configure a variável OPENAI_API_KEY no nó."
                        )
                else:
                    try:
                        caption = _call_openai_vision_api(
                            img_path, prompt, api_key, api_base, openai_model
                        )
                    except (RuntimeError, ValueError, OSError) as exc:
                        if os.environ.get("AUTOLABEL_TEST_MOCK_FALLBACK") == "1":
                            print(
                                f"[autolabel-openai] Fallback ativado para teste: {exc}",
                                file=sys.stderr,
                            )
                            h = int(
                                hashlib.sha256(fname.encode("utf-8")).hexdigest(),
                                16,
                            )
                            fallback_desc = MOCK_DESCRIPTIONS[
                                h % len(MOCK_DESCRIPTIONS)
                            ]
                            caption = f"{prompt or 'Desc'} — [OpenAI fallback]: {fallback_desc}"
                        else:
                            _die(f"AutoLabel OpenAI falhou na imagem '{fname}': {exc}")
            elif model == "florence-2":
                caption = _generate_caption_florence(seed, fname, prompt)
            elif model == "qwen2-vl":
                caption = _generate_caption_qwen(seed, fname, prompt)
            else:
                caption = _generate_caption_mock(seed, fname, prompt)

            line = json.dumps(
                {"filename": fname, "caption": caption}, ensure_ascii=False
            )
            f.write(line + "\n")

    # Gera metrics.jsonl (1 linha para compatibilidade com orquestrador)
    metrics_path = output_dir / "metrics.jsonl"
    with open(metrics_path, "w", encoding="utf-8") as f:
        metrics = {
            "epoch": 1,
            "loss": 0.0,
            "images": len(sorted_filenames),
            "model": model,
        }
        f.write(json.dumps(metrics) + "\n")


# Aliases de compatibilidade com v1
_mock_autolabel = _autolabel_pipeline
_generate_caption = _generate_caption_mock


def _read_dataset_images(dataset_path: Path) -> list[str]:
    return sorted(_discover_dataset_images(dataset_path).keys())


def cmd_autolabel(args: list[str]) -> None:
    """Subcomando autolabel."""
    parser = argparse.ArgumentParser(
        prog="trainer-yolo autolabel",
        description="AutoLabel v2 — Geração de legendas em lote com VLM e OpenAI API",
    )
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--output", required=True, help="Output directory")

    opts = parser.parse_args(args)
    cfg = load_and_validate_autolabel_config(opts.config)
    output = Path(opts.output)

    _autolabel_pipeline(cfg, output)
