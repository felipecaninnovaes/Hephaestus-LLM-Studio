-- 0021_backfill_job_metric_points.sql — backfill único do `jobs.metrics` JSONB
-- legado para `job_metric_points` (spec tasks/specs/telemetria-observabilidade.md
-- §3.1, fatia 0b). Sem data de corte; idempotente via `ON CONFLICT ... DO NOTHING`
-- na chave natural (job_id, key, epoch, step) — rodar duas vezes não altera a
-- contagem. `jobs.metrics` só é removida em migration posterior, após gate manual
-- do orchestrator conferindo a contagem em produção (§5 da spec).
--
-- Formato aceito espelha `normalize_metrics_to_array`
-- (services/manager/src/reporting/metrics.rs:11-22): array, `{"items":[...]}`
-- ou objeto único.
--
-- DESVIO da spec (justificado): o SQL literal do §3.1 faz `(item->>'step')::BIGINT`
-- e `NULLIF(item->>'epoch','')::INTEGER` sem checar `jsonb_typeof`, e
-- `(kv.value #>> '{}')::DOUBLE PRECISION` sem proteção contra overflow — qualquer
-- item malformado (step/epoch não-numérico, ou um elemento do array que não é
-- objeto) ou um valor numérico fora da faixa de `double precision` aborta a
-- migration inteira (erro de cast não é recuperável em SQL puro). Adicionamos:
-- (1) guardas `jsonb_typeof(...) = 'number'` antes de cada cast de step/epoch,
-- com fallback (`step` default 0, `epoch` NULL); (2) `jsonb_each` só roda sobre
-- `item` quando `jsonb_typeof(item) = 'object'` (array com elemento não-objeto
-- vira conjunto vazio, não erro); (3) uma função auxiliar `safe_jsonb_to_double`
-- com `EXCEPTION WHEN OTHERS` para absorver overflow de double precision,
-- descartando (não abortando) o ponto malformado. A função é removida ao fim
-- desta mesma migration — não fica residual no schema.

CREATE FUNCTION pg_temp_safe_jsonb_to_double(v jsonb) RETURNS double precision AS $$
BEGIN
    RETURN (v #>> '{}')::double precision;
EXCEPTION WHEN OTHERS THEN
    RETURN NULL;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

WITH exploded AS (
    SELECT j.id AS job_id,
           CASE WHEN jsonb_typeof(item->'epoch') = 'number'
                THEN (item->>'epoch')::INTEGER ELSE NULL END AS epoch,
           CASE WHEN jsonb_typeof(item->'step') = 'number'
                THEN (item->>'step')::BIGINT ELSE 0 END AS step,
           kv.key,
           pg_temp_safe_jsonb_to_double(kv.value) AS value
    FROM jobs j
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE jsonb_typeof(j.metrics)
            WHEN 'array'  THEN j.metrics
            WHEN 'object' THEN COALESCE(j.metrics->'items', jsonb_build_array(j.metrics))
            ELSE '[]'::jsonb END) AS item
    CROSS JOIN LATERAL jsonb_each(
        CASE WHEN jsonb_typeof(item) = 'object' THEN item ELSE '{}'::jsonb END) AS kv
    WHERE kv.key NOT IN ('epoch', 'step')
      AND jsonb_typeof(kv.value) = 'number'
), numbered AS (
    SELECT e.*, ROW_NUMBER() OVER (PARTITION BY job_id ORDER BY epoch NULLS FIRST, step, key) AS seq
    FROM exploded e
    WHERE e.value IS NOT NULL
)
INSERT INTO job_metric_points (job_id, seq, epoch, step, key, value)
SELECT job_id, seq, epoch, step, key, value FROM numbered
ON CONFLICT ON CONSTRAINT job_metric_points_natural_key DO NOTHING;

UPDATE jobs j SET metric_seq = s.max_seq
FROM (SELECT job_id, MAX(seq) AS max_seq FROM job_metric_points GROUP BY job_id) s
WHERE j.id = s.job_id;

DROP FUNCTION pg_temp_safe_jsonb_to_double(jsonb);

-- ---------------------------------------------------------------------------
-- Query de verificação (fatia 0b, aceite): compara, por job, a contagem de
-- pontos persistidos em job_metric_points com a contagem de pares numéricos
-- (fora epoch/step) no JSONB legado. Rodar manualmente após aplicar a
-- migration; não faz parte do `UP` (só leitura).
-- ---------------------------------------------------------------------------
-- WITH exploded AS (
--     SELECT j.id AS job_id,
--            kv.key,
--            kv.value
--     FROM jobs j
--     CROSS JOIN LATERAL jsonb_array_elements(
--         CASE jsonb_typeof(j.metrics)
--             WHEN 'array'  THEN j.metrics
--             WHEN 'object' THEN COALESCE(j.metrics->'items', jsonb_build_array(j.metrics))
--             ELSE '[]'::jsonb END) AS item
--     CROSS JOIN LATERAL jsonb_each(
--         CASE WHEN jsonb_typeof(item) = 'object' THEN item ELSE '{}'::jsonb END) AS kv
--     WHERE kv.key NOT IN ('epoch', 'step')
--       AND jsonb_typeof(kv.value) = 'number'
-- )
-- SELECT e.job_id,
--        COUNT(*) AS legacy_numeric_pairs,
--        (SELECT COUNT(*) FROM job_metric_points p WHERE p.job_id = e.job_id) AS points_table_count
-- FROM exploded e
-- GROUP BY e.job_id
-- ORDER BY e.job_id;
