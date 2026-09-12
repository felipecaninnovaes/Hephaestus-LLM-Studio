-- 0010_models_engines.sql — suporte a novas engines no catálogo de modelos (diffusion, clip).
-- Dono: manager. Expande models_engine_check para incluir 'diffusion' e 'clip'.

ALTER TABLE models DROP CONSTRAINT models_engine_check;
ALTER TABLE models ADD CONSTRAINT models_engine_check
    CHECK (engine IN ('yolo', 'world', 'diffusion', 'clip'));
