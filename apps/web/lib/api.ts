export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

interface ErrorEnvelope {
  code?: string;
  message?: string;
}

/** RequestInit com body que também aceita objeto plain (serializado como JSON). */
export interface ApiInit extends Omit<RequestInit, "body"> {
  body?: BodyInit | object | null;
}

function isPlainJsonBody(body: unknown): boolean {
  if (body == null) return false;
  if (typeof body === "string") return false;
  if (typeof FormData !== "undefined" && body instanceof FormData) return false;
  if (typeof Blob !== "undefined" && body instanceof Blob) return false;
  if (typeof URLSearchParams !== "undefined" && body instanceof URLSearchParams)
    return false;
  if (body instanceof ArrayBuffer) return false;
  if (ArrayBuffer.isView(body)) return false;
  return typeof body === "object";
}

export async function apiFetch<T>(path: string, init?: ApiInit): Promise<T> {
  const headers = new Headers(init?.headers);
  let body = init?.body;
  if (isPlainJsonBody(body)) {
    body = JSON.stringify(body);
    if (!headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
  }
  const res = await fetch(path, {
    ...init,
    headers,
    body: body as BodyInit | undefined,
    credentials: "same-origin",
  });
  if (!res.ok) {
    let code = "internal";
    let message = "";
    try {
      const envelope = (await res.json()) as ErrorEnvelope;
      if (typeof envelope.code === "string" && envelope.code) code = envelope.code;
      if (typeof envelope.message === "string") message = envelope.message;
    } catch {
      // Corpo ilegível — mantém code/message padrão.
    }
    throw new ApiError(res.status, code, message);
  }
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}
