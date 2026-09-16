-- 0014_models_diffusion_backfill.sql — Bug 009: o hook pós-treino do manager
-- registrava artefatos de treino de difusão com kind/arch NULL (INSERT sem as
-- colunas + ON CONFLICT DO NOTHING), e a geração rejeita customModelId sem
-- kind='checkpoint'/arch (api-principal jobs/handlers + manager lib).
-- Backfill IDEMPOTENTE e NÃO-DESTRUTIVO: só preenche NULL (WHERE IS NULL +
-- COALESCE), nunca sobrescreve valores existentes, nunca apaga rows.
-- Rows sem arch derivável ficam como estão (sem chute).

-- Deriva o arch canônico de cada job de treino de difusão.
-- Fontes por prioridade: params.baseModel (camelCase do BFF) →
-- params.base_model (legado) → jobs.model (= base_model do treino) →
-- linha model: do config_yaml gerado pelo api-principal.
WITH derived AS (
    SELECT
        j.id AS job_id,
        CASE lower(COALESCE(
            NULLIF(j.params->>'baseModel', ''),
            NULLIF(j.params->>'base_model', ''),
            NULLIF(j.model, ''),
            COALESCE(
                substring(j.config_yaml from '(?:^|\n)\s*model:\s*"([^"]+)"'),
                substring(j.config_yaml from '(?:^|\n)\s*model:\s*([A-Za-z0-9._-]+)')
            ),
            ''
        ))
            WHEN 'sdxl' THEN 'sdxl'
            WHEN 'sd15' THEN 'sd15'
            WHEN 'sd1.5' THEN 'sd15'
            WHEN 'sd_15' THEN 'sd15'
            WHEN 'sd-15' THEN 'sd15'
            WHEN 'sd 15' THEN 'sd15'
            WHEN 'flux' THEN 'flux-2-klein-4b'
            WHEN 'flux2' THEN 'flux-2-klein-4b'
            WHEN 'flux-2-klein' THEN 'flux-2-klein-4b'
            WHEN 'flux2-klein-4b' THEN 'flux-2-klein-4b'
            WHEN 'flux-2-klein-4b' THEN 'flux-2-klein-4b'
            ELSE NULL
        END AS arch
    FROM jobs j
    WHERE j.engine = 'diffusion'
      AND (j.mode = 'train' OR j.kind = 'diffusion_train')
)
-- 1. Rows sem kind vindas de treino de difusão: adapter/lora → 'lora',
--    demais artefatos finais → 'checkpoint'. Só quando arch derivável.
UPDATE models m
SET kind = CASE
        WHEN m.s3_key ILIKE '%adapter%' OR m.s3_key ILIKE '%lora%' THEN 'lora'
        ELSE 'checkpoint'
    END,
    arch = COALESCE(m.arch, d.arch)
FROM derived d
WHERE m.job_id = d.job_id
  AND m.kind IS NULL
  AND m.engine = 'diffusion'
  AND m.source = 'train'
  AND d.arch IS NOT NULL;

-- 2. Rows já classificadas (ex.: 'lora' da 0011) mas sem arch: preenche arch
-- derivável do job de treino. Não toca em arch já definido.
WITH derived AS (
    SELECT
        j.id AS job_id,
        CASE lower(COALESCE(
            NULLIF(j.params->>'baseModel', ''),
            NULLIF(j.params->>'base_model', ''),
            NULLIF(j.model, ''),
            COALESCE(
                substring(j.config_yaml from '(?:^|\n)\s*model:\s*"([^"]+)"'),
                substring(j.config_yaml from '(?:^|\n)\s*model:\s*([A-Za-z0-9._-]+)')
            ),
            ''
        ))
            WHEN 'sdxl' THEN 'sdxl'
            WHEN 'sd15' THEN 'sd15'
            WHEN 'sd1.5' THEN 'sd15'
            WHEN 'sd_15' THEN 'sd15'
            WHEN 'sd-15' THEN 'sd15'
            WHEN 'sd 15' THEN 'sd15'
            WHEN 'flux' THEN 'flux-2-klein-4b'
            WHEN 'flux2' THEN 'flux-2-klein-4b'
            WHEN 'flux-2-klein' THEN 'flux-2-klein-4b'
            WHEN 'flux2-klein-4b' THEN 'flux-2-klein-4b'
            WHEN 'flux-2-klein-4b' THEN 'flux-2-klein-4b'
            ELSE NULL
        END AS arch
    FROM jobs j
    WHERE j.engine = 'diffusion'
      AND (j.mode = 'train' OR j.kind = 'diffusion_train')
)
UPDATE models m
SET arch = d.arch
FROM derived d
WHERE m.job_id = d.job_id
  AND m.arch IS NULL
  AND m.engine = 'diffusion'
  AND d.arch IS NOT NULL;
