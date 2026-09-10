-- 0007_models.sql — tabela `models` (catálogo canônico de pesos; ADR-0012 D1/D2).
-- Dono: manager. Bytes: bucket (models/<engine>/<id>/<name> p/ upload/download;
-- artifacts/<job_id>/<path> p/ checkpoints de treino — o hook NÃO copia bytes).

CREATE TABLE models (
    id UUID PRIMARY KEY,
    engine TEXT NOT NULL CHECK (engine IN ('yolo')),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255),
    model TEXT,
    s3_key TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL CHECK (source IN ('train','upload','download')),
    url TEXT,
    hash TEXT NOT NULL CHECK (hash ~ '^[0-9a-f]{32}$'),
    bytes BIGINT NOT NULL CHECK (bytes >= 0),
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX models_engine_idx ON models(engine);
CREATE INDEX models_created_at_idx ON models(created_at DESC);

-- Backfill (D2): checkpoints best.pt de treinos done existentes. Idempotente.
INSERT INTO models (id, engine, name, model, s3_key, source, hash, bytes, job_id, created_at)
SELECT ja.id, j.engine, split_part(ja.path, '/', -1), j.model,
       'artifacts/' || ja.job_id::text || '/' || ja.path, 'train',
       ja.md5, ja.bytes, ja.job_id, j.created_at
FROM job_artifacts ja
JOIN jobs j ON j.id = ja.job_id
WHERE ja.kind = 'model' AND j.status = 'done' AND ja.path LIKE '%best%'
ON CONFLICT (s3_key) DO NOTHING;
