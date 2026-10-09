-- 0026_comfy_targets.sql — destinos ComfyUI e exportações de LoRA (feat/comfyui-export).
-- Dono: api-principal. Segredo do destino (token do custom node) fica cifrado
-- (AES-256-GCM, chave derivada de auth_state.jwt_secret via HKDF) — nunca em claro.
-- Exportações em `queued`/`uploading` viram `failed` no boot do BFF (a task morre com o processo).

CREATE TABLE comfy_targets (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK (char_length(name) BETWEEN 1 AND 100),
    base_url TEXT NOT NULL CHECK (char_length(base_url) BETWEEN 1 AND 2048),
    token_ciphertext BYTEA NOT NULL,
    token_nonce BYTEA NOT NULL CHECK (octet_length(token_nonce) = 12),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_check_at TIMESTAMPTZ,
    last_check_ok BOOLEAN,
    last_check_error TEXT
);

CREATE TABLE comfy_exports (
    id UUID PRIMARY KEY,
    target_id UUID NOT NULL REFERENCES comfy_targets(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('model', 'artifact')),
    model_id UUID,
    job_id UUID,
    artifact_id UUID,
    filename TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
    status TEXT NOT NULL CHECK (status IN ('queued', 'uploading', 'done', 'failed')),
    bytes_total BIGINT NOT NULL DEFAULT 0 CHECK (bytes_total >= 0),
    bytes_sent BIGINT NOT NULL DEFAULT 0 CHECK (bytes_sent >= 0),
    error TEXT,
    error_code TEXT,
    remote_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (source_kind = 'model' AND model_id IS NOT NULL)
        OR (source_kind = 'artifact' AND job_id IS NOT NULL AND artifact_id IS NOT NULL)
    )
);

CREATE INDEX comfy_exports_created_at_idx ON comfy_exports (created_at DESC);
CREATE INDEX comfy_exports_target_idx ON comfy_exports (target_id);
