CREATE TABLE job_alerts (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id      UUID        NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    rule_id     TEXT        NOT NULL CHECK (rule_id IN ('nan_detected','vram_high','disk_high','telemetry_stale')),
    severity    TEXT        NOT NULL CHECK (severity IN ('warning','critical')),
    message     TEXT        NOT NULL,
    fired_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ
);

-- dedupe: no máximo um alerta ativo por (job, regra)
CREATE UNIQUE INDEX job_alerts_active_uniq
    ON job_alerts (job_id, rule_id) WHERE resolved_at IS NULL;

CREATE INDEX job_alerts_job_fired_idx ON job_alerts (job_id, fired_at DESC);

-- início real da execução (1º report running); referência de telemetry_stale sem pontos
ALTER TABLE jobs ADD COLUMN started_at TIMESTAMPTZ;
