"""
Integração com APIs compatíveis com OpenAI Vision / VLM para geração de legendas de imagens.
"""
from __future__ import annotations

import base64
import io
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


# Erros HTTP que indicam configuração inválida (chave/endpoint/modelo), não falha por imagem.
_FATAL_HTTP_STATUS = frozenset({401, 403, 404})


class VisionApiFatalError(RuntimeError):
    """Erro de API que afeta o job inteiro (HTTP 401/403/404); não deve ser pulado."""

    def __init__(self, message: str, status: int) -> None:
        super().__init__(message)
        self.status = status


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


def _prepare_image_for_vision(
    image_path: Path, max_dimension: int = 1024
) -> tuple[str, str]:
    """Prepara a imagem para VLM: redimensiona mantendo aspect ratio se maior que max_dimension e converte para base64 JPEG."""
    try:
        from PIL import Image

        with Image.open(image_path) as im:
            w, h = im.size
            if max(w, h) > max_dimension:
                im.thumbnail((max_dimension, max_dimension), Image.Resampling.LANCZOS)
            if im.mode not in ("RGB", "L"):
                im = im.convert("RGB")
            buf = io.BytesIO()
            im.save(buf, format="JPEG", quality=85, optimize=True)
            jpeg_bytes = buf.getvalue()
            b64_img = base64.b64encode(jpeg_bytes).decode("utf-8")
            return b64_img, "image/jpeg"
    except Exception:
        image_bytes = image_path.read_bytes()
        b64_img = base64.b64encode(image_bytes).decode("utf-8")
        return b64_img, _get_mime_type(image_path)


_THINK_BLOCK_RE = re.compile(r"<think>.*?</think>", re.IGNORECASE | re.DOTALL)
_THINK_OPEN_RE = re.compile(r"<think>", re.IGNORECASE)


def _positive_int_env(name: str) -> int | None:
    """Lê um inteiro positivo do ambiente; ausente, inválido ou <= 0 ⇒ None."""
    raw = os.environ.get(name)
    if raw is None:
        return None
    try:
        value = int(raw.strip())
    except ValueError:
        return None
    return value if value > 0 else None


def _strip_thinking(text: str) -> str:
    """Remove blocos <think>...</think>; um <think> sem fechamento descarta o resto."""
    text = _THINK_BLOCK_RE.sub("", text)
    match = _THINK_OPEN_RE.search(text)
    if match:
        text = text[: match.start()]
    return text.strip()


def _extract_caption(choice: dict) -> str:
    """Extrai a resposta final de choices[0]; nunca usa campos de raciocínio.

    Levanta RuntimeError se a resposta final estiver vazia ou truncada.
    """
    message = choice.get("message") or {}
    content = message.get("content", "")
    if isinstance(content, list):
        content = " ".join(
            p.get("text", "")
            for p in content
            if isinstance(p, dict) and p.get("type") == "text"
        )
    answer = _strip_thinking(str(content)) if content else ""
    truncated = choice.get("finish_reason") == "length"
    if not answer:
        if truncated:
            raise RuntimeError(
                "O modelo esgotou o limite de tokens durante o raciocínio e não "
                "produziu resposta final (finish_reason=length). Aumente "
                "AUTOLABEL_MAX_TOKENS ou o contexto do servidor, ou desative o raciocínio."
            )
        raise RuntimeError("O modelo não retornou resposta final (legenda vazia).")
    if truncated:
        raise RuntimeError(
            "A legenda foi truncada pelo limite de tokens (finish_reason=length). "
            "Aumente AUTOLABEL_MAX_TOKENS ou o contexto do servidor, ou desative o raciocínio."
        )
    return answer


def _call_openai_vision_api(
    image_path: Path,
    prompt: str | None,
    api_key: str | None,
    api_base: str,
    openai_model: str,
    reasoning_effort: str | None = None,
) -> str:
    """Faz chamada HTTP à API compatível com OpenAI Vision para descrever a imagem."""
    norm_base = _normalize_api_base(api_base)
    b64_img, mime = _prepare_image_for_vision(image_path)

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
    }
    max_tokens = _positive_int_env("AUTOLABEL_MAX_TOKENS")
    if max_tokens is not None:
        payload["max_tokens"] = max_tokens
    if reasoning_effort:
        payload["reasoning_effort"] = reasoning_effort

    url = f"{norm_base}/chat/completions"
    data = json.dumps(payload).encode("utf-8")

    headers = {
        "Content-Type": "application/json",
        "User-Agent": "Hephaestus-Studio-AutoLabel/2.0",
    }
    if api_key and api_key.strip():
        headers["Authorization"] = f"Bearer {api_key.strip()}"

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

    request_timeout = _positive_int_env("AUTOLABEL_REQUEST_TIMEOUT") or 300
    max_retries = 2
    for attempt in range(max_retries + 1):
        try:
            with urllib.request.urlopen(
                req, timeout=request_timeout, context=ctx
            ) as resp:
                body = json.loads(resp.read().decode("utf-8"))
                choices = body.get("choices")
                if not choices or not isinstance(choices, list):
                    raise ValueError(f"Formato de resposta inesperado da API: {body}")
                return _extract_caption(choices[0])
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

            if exc.code in _FATAL_HTTP_STATUS:
                raise VisionApiFatalError(
                    f"OpenAI API error ({url}): {err_msg}", status=exc.code
                ) from exc
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
        except RuntimeError:
            raise
        except Exception as exc:
            print(
                f"[autolabel-openai] Erro inesperado para {image_path.name}: {exc}",
                file=sys.stderr,
            )
            raise RuntimeError(f"OpenAI API parse error: {exc}") from exc

    raise RuntimeError(
        f"OpenAI API falhou após {max_retries + 1} tentativas para {image_path.name}"
    )
