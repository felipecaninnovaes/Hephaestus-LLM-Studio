-- 0020_job_metric_points.sql — tabela append-only de pontos de série temporal
-- de métricas (spec tasks/specs/telemetria-observabilidade.md §2.2/§3.1, fatia 0b).
-- Dono: api-principal. Substitui gradualmente `jobs.metrics` (JSONB, O(n²) por
-- report): o insert é feito pelo manager (fatia 1a) com `seq` alocada via
-- `UPDATE jobs SET metric_seq = metric_seq + $n ... RETURNING metric_seq`, o
-- que serializa reports concorrentes do mesmo job pelo lock de linha.
-- Idempotência (outbox at-least-once): chave natural (job_id, key, epoch, step)
-- com NULLS NOT DISTINCT — epoch nulo conta como valor igual a outro epoch nulo,
-- então um ponto sem epoch não é mais descartado em silêncio como no legado
-- (`metrics.rs:26-30` exige epoch para computar `metrics_key`).
-- Sem DOWN dedicado: o repo usa sqlx migrate! embarcado só-up (main.rs).

ALTER TABLE jobs ADD COLUMN metric_seq BIGINT NOT NULL DEFAULT 0;

CREATE TABLE job_metric_points (
    job_id  UUID             NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    seq     BIGINT           NOT NULL,
    epoch   INTEGER,                       -- nullable: ponto sem epoch não é mais descartado
    step    BIGINT           NOT NULL,
    key     TEXT             NOT NULL,
    value   DOUBLE PRECISION NOT NULL,
    ts      TIMESTAMPTZ      NOT NULL DEFAULT now(),
    PRIMARY KEY (job_id, seq),
    CONSTRAINT job_metric_points_natural_key
        UNIQUE NULLS NOT DISTINCT (job_id, key, epoch, step)
);

CREATE INDEX job_metric_points_job_key_step_idx
    ON job_metric_points (job_id, key, step);
