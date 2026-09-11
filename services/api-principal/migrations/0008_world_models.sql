-- 0008_world_models.sql — AutoTracker real (ADR-0014 D1): engine 'world' no catálogo.
-- Dono: manager. O mundo world (yolov8x-worldv2.pt) é um peso do ecossistema
-- ultralytics; a row nasce com engine='world' para o fine-tune yolo continuar
-- recusando-o (400 na resolução de weights_id — ADR-0012 D5) e o autotracker
-- real aceitá-lo (ADR-0014 D5). Sem trigger e sem índice novo: o filtro por
-- engine usa `models_engine_idx` existente (0007).

ALTER TABLE models DROP CONSTRAINT models_engine_check;
ALTER TABLE models ADD CONSTRAINT models_engine_check
    CHECK (engine IN ('yolo', 'world'));
