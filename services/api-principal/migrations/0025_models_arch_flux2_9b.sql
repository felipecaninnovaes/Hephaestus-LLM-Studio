-- 0025_models_arch_flux2_9b.sql — arch 'flux-2-klein-9b' no catálogo de modelos.
-- Dono: api-principal (fatia feat/flux2-klein-9b-be). Expande o CHECK de arch
-- da migration 0019 para aceitar 'flux-2-klein-9b' (FLUX.2 Klein base 9B).
-- Não-destrutivo: só alarga o domínio; rows existentes preservadas.
-- Deve rodar ANTES do primeiro treino 9B (o manager registra o modelo com
-- arch=flux-2-klein-9b ao fim do treino).

ALTER TABLE models DROP CONSTRAINT models_arch_check;
ALTER TABLE models ADD CONSTRAINT models_arch_check
    CHECK (arch IS NULL OR arch IN ('flux-2-klein-4b', 'flux-2-klein-9b', 'sdxl', 'sd15', 'qwen-image-2.1'));
