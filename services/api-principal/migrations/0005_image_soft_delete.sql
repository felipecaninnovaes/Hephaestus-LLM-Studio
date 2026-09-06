-- Fatia 3g / ADR-0005. Exclusão de imagem restaurável (soft-delete): coluna deleted_at, unique parcial de filename só sobre ativas, contadores ignoram lixeira.

ALTER TABLE images ADD COLUMN deleted_at TIMESTAMPTZ NULL;

ALTER TABLE images DROP CONSTRAINT IF EXISTS images_dataset_id_filename_key;

CREATE UNIQUE INDEX images_dataset_id_filename_active_uniq
    ON images (dataset_id, filename) WHERE deleted_at IS NULL;

CREATE INDEX images_dataset_id_deleted_idx
    ON images (dataset_id) WHERE deleted_at IS NOT NULL;

CREATE OR REPLACE FUNCTION heph_refresh_dataset_counters(p_dataset_id uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_format   TEXT;
    v_images   INT;
    v_labeled  INT;
    v_size     BIGINT;
    v_status   TEXT;
BEGIN
    SELECT format INTO v_format FROM datasets WHERE id = p_dataset_id;
    IF NOT FOUND THEN RETURN; END IF;   -- dataset em delete: UPDATE não casaria mesmo

    SELECT count(*), COALESCE(sum(bytes), 0) INTO v_images, v_size FROM images WHERE dataset_id = p_dataset_id AND deleted_at IS NULL;
    -- videos entram no tamanho (sem rota de escrita na 3b, zero hoje):
    SELECT v_size + COALESCE(sum(bytes), 0) INTO v_size FROM videos WHERE dataset_id = p_dataset_id;

    -- "labeled" respeita a taxonomia de format (ADR-0002 D3 / ADR-0003 R9):
    --   yolo_txt  -> imagem com >=1 box (caption solto NÃO conta)
    --   demais    -> imagem com linha em captions (box NÃO conta)
    SELECT count(*) INTO v_labeled
    FROM images i
    WHERE i.dataset_id = p_dataset_id
      AND i.deleted_at IS NULL
      AND (
        (v_format = 'yolo_txt' AND EXISTS (SELECT 1 FROM boxes b WHERE b.image_id = i.id))
        OR
        (v_format <> 'yolo_txt' AND EXISTS (SELECT 1 FROM captions c WHERE c.image_id = i.id))
      );

    v_status := CASE
        WHEN v_images = 0      THEN 'needs_labeling'
        WHEN v_labeled = 0     THEN 'needs_labeling'
        WHEN v_labeled = v_images THEN 'ready'
        ELSE 'in_progress'
    END;

    -- guarda IS DISTINCT FROM: sem mudança real, sem UPDATE, sem churn de updated_at
    UPDATE datasets SET
        images_count  = v_images,
        labeled_count = v_labeled,
        size_bytes    = v_size,
        status        = v_status
    WHERE id = p_dataset_id
      AND (images_count, labeled_count, size_bytes, status)
          IS DISTINCT FROM (v_images, v_labeled, v_size, v_status);
END;
$$;
