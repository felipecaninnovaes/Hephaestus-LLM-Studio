-- 0019_models_arch_qwen.sql — arch 'qwen-image-2.1' no catálogo de modelos.
-- Dono: api-principal (fatia feat/engine-qwen-image-2-1). Expande o CHECK de arch
-- da migration 0011 para aceitar 'qwen-image-2.1'.
-- Não-destrutivo: só alarga o domínio; rows existentes preservadas.

ALTER TABLE models DROP CONSTRAINT models_arch_check;
ALTER TABLE models ADD CONSTRAINT models_arch_check
    CHECK (arch IS NULL OR arch IN ('flux-2-klein-4b', 'sdxl', 'sd15', 'qwen-image-2.1'));
