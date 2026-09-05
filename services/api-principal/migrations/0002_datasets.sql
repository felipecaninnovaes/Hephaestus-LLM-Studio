-- Fatia 3a / ADR-0002. Colunas seguem backend.md §10 à letra; os CHECKs de domínio
-- e o UNIQUE(dataset_id, idx) são APERTO de semântica sobre o §10 (tensão T1 do
-- ADR-0002), não tabelas novas. gen_random_uuid() é NATIVO desde o PG13
-- (postgres:16) — NÃO requer pgcrypto.

-- updated_at está AUSENTE no §10: adicionado pela ADR-0002 D4 porque
-- frontend.md §5.1 pede `lastModified` e mapear created_at mentiria a partir da
-- 1ª edição (3b/3c). Mantido por trigger BEFORE UPDATE.
CREATE TABLE datasets (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug          TEXT    NOT NULL UNIQUE,
    title         TEXT    NOT NULL CHECK (char_length(title) BETWEEN 1 AND 96),
    category      TEXT    NOT NULL CHECK (category IN ('difusao','openclip','yolo')),
    type          TEXT    NOT NULL CHECK (type IN ('yolo_bbox','yolo_seg','difusao_lora','clip_image_text')),
    task          TEXT    NOT NULL CHECK (task IN ('detect_track','segment','caption','embedding')),
    format        TEXT    NOT NULL CHECK (format IN ('yolo_txt','captions','pairs')),
    status        TEXT    NOT NULL CHECK (status IN ('ready','in_progress','needs_labeling')),
    source        TEXT    NULL,
    size_bytes    BIGINT  NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    images_count  INT     NOT NULL DEFAULT 0 CHECK (images_count >= 0),
    labeled_count INT     NOT NULL DEFAULT 0 CHECK (labeled_count >= 0),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sem índice extra em datasets: a 3a lê a coleção inteira e o UNIQUE(slug) já
-- gera índice; o §10 só nomeia índices para images/boxes/jobs.
-- size_bytes/images_count/labeled_count NÃO têm caminho de escrita na 3a:
-- 0 é teorema, não estimativa (ADR-0002 T2). O invariante `% Rotuladas`
-- (`labeled_count <= images_count`) é OBRIGAÇÃO do trigger da 3b — garantido
-- por uma única função de recálculo dos dois contadores ou por
-- `CONSTRAINT TRIGGER ... DEFERRABLE INITIALLY DEFERRED`, NUNCA por CHECK:
-- o `DELETE FROM images` de uma imagem rotulada passa por estado
-- intermediário (trigger que baixa `images_count` vs. cascata que baixa
-- `labeled_count`, ordem de triggers de RI vs. de usuário) que violaria o
-- CHECK, e CHECK não é deferrável no Postgres (ADR-0002 T2).

-- Trigger genérico de updated_at (plpgsql puro, sem extensão nova) — será
-- reaproveitado por captions/images nas fatias seguintes.
CREATE FUNCTION tg_set_updated_at() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

CREATE TRIGGER datasets_set_updated_at
    BEFORE UPDATE ON datasets
    FOR EACH ROW EXECUTE FUNCTION tg_set_updated_at();

CREATE TABLE classes (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    dataset_id UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (name ~ '^[A-Za-z0-9_][A-Za-z0-9_]{0,63}$'),
    idx        INT  NOT NULL CHECK (idx >= 0),
    color      TEXT NOT NULL CHECK (color ~ '^#[0-9a-f]{6}$'),
    UNIQUE (dataset_id, name),
    UNIQUE (dataset_id, idx)
);

-- Índice do JOIN da lista (uma linha por dataset, ordenada por idx).
-- Implementação do acesso, não schema novo: o §10 não nomeia este índice.
CREATE INDEX classes_dataset_id_idx ON classes (dataset_id, idx);
