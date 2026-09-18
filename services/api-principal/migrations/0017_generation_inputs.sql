-- 0017_generation_inputs.sql — inputs efêmeros para img2img (gerador de difusão).
-- Dono: api-principal. Upload avulso de imagem inicial via `POST /api/generations/inputs`
-- (objeto sob o prefixo S3 `generation_inputs/`); a linha é referenciada como
-- `initImageId` em `POST /api/jobs/diffusion/generate` e marcada em `used_at` ao ser
-- consumida. Recurso efêmero, sem GC automático: linhas consumidas permanecem p/ auditoria.
-- Alternativa rejeitada: reutilizar `images`/`datasets` (acoplaria o gerador ao domínio
-- de datasets) ou só aceitar `initGenerationId` (exigiria persistir uploads avulsos na galeria).
CREATE TABLE IF NOT EXISTS generation_inputs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  s3_key text NOT NULL UNIQUE,
  filename text NOT NULL,
  mime_type text NOT NULL,
  width integer NOT NULL,
  height integer NOT NULL,
  md5 char(32) NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  used_at timestamptz
);
