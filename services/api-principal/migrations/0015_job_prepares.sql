-- 0015_job_prepares.sql — prepare state da submissão assíncrona (ADR-0025 D3, fatia P2).
-- Dono: principal. Uma linha por job aceito em `preparing` (202 antes do pacote
-- existir); o worker de background avança preparing → done/failed/cancelled.
-- Dedupe por (dataset_id, fingerprint, state); recovery no boot re-spawna
-- `preparing` com attempts < 3; watchdog do manager falha `preparing` > 60min.
-- Sem DOWN dedicado: o repo usa sqlx migrate! embarcado só-up (main.rs).

CREATE TABLE job_prepares (
  job_id UUID PRIMARY KEY,
  dataset_id UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
  fingerprint TEXT NOT NULL,
  spec JSONB NOT NULL DEFAULT '{}'::jsonb,
  state TEXT NOT NULL DEFAULT 'preparing' CHECK (state IN ('preparing','done','failed','cancelled')),
  attempts INT NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX job_prepares_fp ON job_prepares(dataset_id, fingerprint, state);
