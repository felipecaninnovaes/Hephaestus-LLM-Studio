-- 0011_generations.sql — tabela generations + colunas kind/arch no catálogo de modelos.
-- Dono: manager. Expande models com kind (lora/checkpoint) e arch (flux-2-klein-4b/sdxl/sd15);
-- cria tabela generations para galeria persistente de imagens geradas.

ALTER TABLE models
  ADD COLUMN kind TEXT CHECK (kind IS NULL OR kind IN ('lora','checkpoint')),
  ADD COLUMN arch TEXT CHECK (arch IS NULL OR arch IN ('flux-2-klein-4b','sdxl','sd15'));
UPDATE models SET kind = 'lora' WHERE engine = 'diffusion';

CREATE TABLE generations (
  id UUID PRIMARY KEY,
  job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  s3_key TEXT NOT NULL UNIQUE,
  thumb_s3_key TEXT,
  filename TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
  seed BIGINT NOT NULL CHECK (seed >= 0),
  prompt TEXT NOT NULL CHECK (char_length(prompt) BETWEEN 1 AND 4000),
  negative_prompt TEXT CHECK (negative_prompt IS NULL OR char_length(negative_prompt) <= 4000),
  width INT NOT NULL CHECK (width BETWEEN 256 AND 2048),
  height INT NOT NULL CHECK (height BETWEEN 256 AND 2048),
  params JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at TIMESTAMPTZ
);
CREATE INDEX generations_created_at_idx ON generations (created_at DESC);
CREATE INDEX generations_job_id_idx ON generations (job_id);
CREATE INDEX generations_deleted_idx ON generations (deleted_at) WHERE deleted_at IS NOT NULL;
