-- 0016_job_prepares_dedupe.sql — índice único anti-TOCTOU do dedupe (ADR-0025 D3, B1 reviewer)
-- Dono: api-principal. O dedupe de submits concorrentes (B1 da revisão de 2f7077d)
-- exige unicidade real: dois aceites simultâneos com o mesmo fingerprint disputam
-- o INSERT em `accept_job_preparing`; o perdedor recebe 0 linhas
-- (`ON CONFLICT DO NOTHING`), aborta o job recém-criado e responde 202 com o
-- jobId vencedor — sem isso, 2 submits paralelos criavam 2 jobs + 2 builds.
-- Nota: publicado como 0016 (não edição da 0015) porque a 0015 já havia sido
-- aplicada com outro checksum em bancos de dev — sqlx rejeita migration mutada.
CREATE UNIQUE INDEX job_prepares_dedupe
  ON job_prepares(dataset_id, fingerprint) WHERE state = 'preparing';
