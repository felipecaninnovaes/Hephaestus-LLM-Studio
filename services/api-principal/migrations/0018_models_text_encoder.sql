-- 0018_models_text_encoder.sql — kind 'text_encoder' no catálogo de modelos.
-- Dono: api-principal (fatia feat/pesos-custom-flux2). Expande o CHECK de kind
-- da migration 0011 (lora/checkpoint) para aceitar 'text_encoder' (text
-- encoders custom de "Modelos & Pesos", usáveis em treino/geração flux-2).
-- Não-destrutivo: só alarga o domínio; rows existentes preservadas.
-- O arch permitido para text_encoder (só flux-2-klein-4b) é regra de
-- aplicação (validate_create_model no manager + resolve_kind_arch no
-- principal), não CHECK — mesmo padrão do checkpoint (que exige arch na app).

ALTER TABLE models DROP CONSTRAINT models_kind_check;
ALTER TABLE models ADD CONSTRAINT models_kind_check
    CHECK (kind IS NULL OR kind IN ('lora', 'checkpoint', 'text_encoder'));
