-- 0012_generations_job_optional.sql — AC-003: apagar um job não pode apagar a
-- galeria de geração do usuário. Os bytes das gerações preservadas vivem sob o
-- MESMO prefixo artifacts/{job_id}/, por isso o delete de job faz sweep por
-- CHAVES EXATAS (object_keys calculado pelo manager) — nunca por prefixo.
-- As linhas generations ficam órfãs de job (SET NULL) mas mantêm s3_key válido
-- para o proxy de imagem.
-- job_id: NOT NULL + ON DELETE CASCADE  →  NULL + ON DELETE SET NULL.

ALTER TABLE generations ALTER COLUMN job_id DROP NOT NULL;
ALTER TABLE generations DROP CONSTRAINT generations_job_id_fkey;
ALTER TABLE generations
  ADD CONSTRAINT generations_job_id_fkey
  FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE SET NULL;
