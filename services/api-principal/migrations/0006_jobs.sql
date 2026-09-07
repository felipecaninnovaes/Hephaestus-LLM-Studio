-- 0006_jobs.sql — jobs/package/materialização (ADR-0007, fatia 4)
-- Ordem: orchestrators → dataset_versions → jobs → job_artifacts (FKs).

CREATE TABLE orchestrators (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  endpoint TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('local', 'remoto')),
  fingerprint TEXT,
  token_hash TEXT,
  gpus JSONB,
  vram_total_gb INT CHECK (vram_total_gb IS NULL OR vram_total_gb >= 0),
  status TEXT NOT NULL DEFAULT 'unknown',
  last_heartbeat TIMESTAMPTZ
);
CREATE INDEX orchestrators_status ON orchestrators(status);

-- Snapshot do dataset no despacho do job (T4; nunca RESTRICT — ON DELETE CASCADE:
-- a versão morre com o dataset). Dono: principal (domínio de dataset).
CREATE TABLE dataset_versions (
  id UUID PRIMARY KEY,
  dataset_id UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
  manifest JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX dataset_versions_dataset ON dataset_versions(dataset_id, created_at);

CREATE TABLE jobs (
  id UUID PRIMARY KEY,
  kind TEXT NOT NULL,
  dataset_id UUID NULL REFERENCES datasets(id) ON DELETE SET NULL,  -- T4
  engine TEXT NOT NULL,
  model TEXT NOT NULL,
  mode TEXT NOT NULL,
  params JSONB NOT NULL DEFAULT '{}'::jsonb,      -- inclui package_ref (D1b)
  config_yaml TEXT,                                -- config.yaml gerado pelo principal
  status TEXT NOT NULL DEFAULT 'queued'
    CHECK (status IN ('queued','dispatched','preparing','running','cancelling',
                      'done','failed','cancelled')),
  queue_reason TEXT,                               -- waiting_vram|waiting_slot|recovered
  orchestrator_id UUID NULL REFERENCES orchestrators(id) ON DELETE SET NULL,
  vram_min_gb INT CHECK (vram_min_gb IS NULL OR vram_min_gb >= 0),
  progress FLOAT CHECK (progress IS NULL OR (progress >= 0 AND progress <= 1)),
  epoch INT CHECK (epoch IS NULL OR epoch >= 0),
  step INT CHECK (step IS NULL OR step >= 0),
  metrics JSONB,                                   -- snake_case (transporte; D1)
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  finished_at TIMESTAMPTZ
);
CREATE INDEX jobs_status ON jobs(status);          -- exigido
CREATE INDEX jobs_dataset ON jobs(dataset_id);
CREATE INDEX jobs_created ON jobs(created_at);

CREATE TABLE job_artifacts (
  id UUID PRIMARY KEY,
  job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  md5 TEXT NOT NULL CHECK (md5 ~ '^[0-9a-f]{32}$'),
  bytes BIGINT NOT NULL CHECK (bytes >= 0)
);
CREATE INDEX job_artifacts_job ON job_artifacts(job_id);  -- exigido
