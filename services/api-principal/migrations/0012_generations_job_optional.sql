-- 0012_generations_job_optional.sql — AC-003: apagar um job não pode apagar a
-- galeria de geração do usuário (os bytes vivem sob artifacts/{job_id}/ e o
-- delete de job faz sweep do prefixo; as linhas generations preservadas ficam
-- órfãs de job mas mantêm s3_key válido para o proxy de imagem).
-- job_id: NOT NULL + ON DELETE CASCADE  →  NULL + ON DELETE SET NULL.

ALTER TABLE generations ALTER COLUMN job_id DROP NOT NULL;
ALTER TABLE generations DROP CONSTRAINT generations_job_id_fkey;
ALTER TABLE generations
  ADD CONSTRAINT generations_job_id_fkey
  FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE SET NULL;
