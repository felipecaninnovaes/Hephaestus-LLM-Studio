-- Fatia 3b / ADR-0003. Blob canônico vira objeto S3; estas tabelas são o índice relacional. Imagens/vídeos: linhas apontam object_key; boxes/captions: a verdade das anotações (D6).

-- D5: a coluna `source` ("caminho em disco") é conceito morto. No wire
-- `Dataset.source` permanece, derivado server-side a partir da 3b.7
-- (até lá sempre `null`). (ADR-0003 D5)
ALTER TABLE datasets DROP COLUMN source;

-- Índice relacional dos objetos S3 (ADR-0003).
-- `object_key` é a chave legível `datasets/{dataset_id}/images/{image_id}/{filename}`
-- (convenção, sem regex imposta aqui).
CREATE TABLE images (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    dataset_id UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    filename   TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
    object_key TEXT NOT NULL UNIQUE,
    bytes      BIGINT NOT NULL CHECK (bytes >= 0),
    width      INT NOT NULL CHECK (width > 0),
    height     INT NOT NULL CHECK (height > 0),
    -- Adição da ADR sobre o §10 (ADR-0003).
    md5        TEXT NOT NULL CHECK (md5 ~ '^[0-9a-f]{32}$'),
    sha256     TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    -- Adição da ADR sobre o §10 (ADR-0003).
    media_type TEXT NOT NULL CHECK (media_type IN ('jpeg','png','webp')),
    split      TEXT NOT NULL DEFAULT 'train' CHECK (split IN ('train','val')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- "mandei o mesmo lote duas vezes" ⇒ duplicate, zero PUT (ADR-0003 D2/D5).
    UNIQUE (dataset_id, filename)
);

CREATE INDEX images_dataset_id_idx ON images (dataset_id);
CREATE INDEX images_dataset_id_split_idx ON images (dataset_id, split);

-- Verdade das anotações de detecção (ADR-0003 D6). Colunas do §10/backend.md à letra.
-- x/y/w/h: centro+dimensões normalizados 0..1.
CREATE TABLE boxes (
    id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    image_id UUID NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    class_id UUID NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    x        DOUBLE PRECISION NOT NULL CHECK (x BETWEEN 0 AND 1),
    y        DOUBLE PRECISION NOT NULL CHECK (y BETWEEN 0 AND 1),
    w        DOUBLE PRECISION NOT NULL CHECK (w BETWEEN 0 AND 1),
    h        DOUBLE PRECISION NOT NULL CHECK (h BETWEEN 0 AND 1),
    conf     DOUBLE PRECISION NULL,
    origin   TEXT NOT NULL CHECK (origin IN ('manual','autotracker','import')),
    track_id INT NULL
);

CREATE INDEX boxes_image_id_idx ON boxes (image_id);
CREATE INDEX boxes_class_id_idx ON boxes (class_id);

-- Verdade das anotações de caption (ADR-0003 D6). PK = image_id: um caption por imagem.
CREATE TABLE captions (
    image_id   UUID PRIMARY KEY REFERENCES images(id) ON DELETE CASCADE,
    text       TEXT NOT NULL CHECK (char_length(text) BETWEEN 1 AND 8000),
    origin     TEXT NOT NULL CHECK (origin IN ('manual','autotracker','import')),
    model      TEXT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Reuso da função da 0002, como ela previu (ADR-0003).
CREATE TRIGGER captions_set_updated_at
    BEFORE UPDATE ON captions
    FOR EACH ROW EXECUTE FUNCTION tg_set_updated_at();

-- D5: nasce agora sem rota de escrita na 3b porque DDL é estático e a FK
-- CASCADE vem junto das irmãs (ADR-0003 D5).
CREATE TABLE videos (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    dataset_id UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    filename   TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
    object_key TEXT NOT NULL UNIQUE,
    fps        DOUBLE PRECISION NULL,
    frames     INT NULL CHECK (frames IS NULL OR frames >= 0),
    md5        TEXT NOT NULL CHECK (md5 ~ '^[0-9a-f]{32}$'),
    bytes      BIGINT NOT NULL CHECK (bytes >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (dataset_id, filename)
);

CREATE INDEX videos_dataset_id_idx ON videos (dataset_id);

-- Contadores — fecha ADR-0002 T2 (ADR-0003).
-- NUNCA `+=`: um único recalculador ⇒ drift impossível. O invariante
-- `labeled_count <= images_count` NÃO é CHECK porque o `DELETE FROM images`
-- de imagem rotulada passa por estado intermediário — a última ordem de
-- triggers sempre vê o estado final (ADR-0002 T2).
CREATE FUNCTION heph_refresh_dataset_counters(p_dataset_id uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    v_format   TEXT;
    v_images   INT;
    v_labeled  INT;
    v_size     BIGINT;
    v_status   TEXT;
BEGIN
    SELECT format INTO v_format FROM datasets WHERE id = p_dataset_id;
    IF NOT FOUND THEN RETURN; END IF;   -- dataset em delete: UPDATE não casaria mesmo

    SELECT count(*), COALESCE(sum(bytes), 0) INTO v_images, v_size FROM images WHERE dataset_id = p_dataset_id;
    -- videos entram no tamanho (sem rota de escrita na 3b, zero hoje):
    SELECT v_size + COALESCE(sum(bytes), 0) INTO v_size FROM videos WHERE dataset_id = p_dataset_id;

    -- "labeled" respeita a taxonomia de format (ADR-0002 D3 / ADR-0003 R9):
    --   yolo_txt  -> imagem com >=1 box (caption solto NÃO conta)
    --   demais    -> imagem com linha em captions (box NÃO conta)
    SELECT count(*) INTO v_labeled
    FROM images i
    WHERE i.dataset_id = p_dataset_id
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

CREATE FUNCTION tg_images_refresh() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM heph_refresh_dataset_counters(COALESCE(NEW.dataset_id, OLD.dataset_id));
    RETURN NULL;
END;
$$;

CREATE FUNCTION tg_videos_refresh() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM heph_refresh_dataset_counters(COALESCE(NEW.dataset_id, OLD.dataset_id));
    RETURN NULL;
END;
$$;

CREATE FUNCTION tg_image_child_refresh() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    v_ds uuid;
BEGIN
    SELECT dataset_id INTO v_ds FROM images WHERE id = COALESCE(NEW.image_id, OLD.image_id);
    -- durante a cascata de delete da mãe o lookup falha e o skip é seguro:
    -- o disparo no nível de `images` já recalculará com o estado final (ADR-0002 T2).
    IF v_ds IS NOT NULL THEN
        PERFORM heph_refresh_dataset_counters(v_ds);
    END IF;
    RETURN NULL;
END;
$$;

CREATE TRIGGER images_refresh_counters
    AFTER INSERT OR UPDATE OR DELETE ON images
    FOR EACH ROW EXECUTE FUNCTION tg_images_refresh();

CREATE TRIGGER videos_refresh_counters
    AFTER INSERT OR UPDATE OR DELETE ON videos
    FOR EACH ROW EXECUTE FUNCTION tg_videos_refresh();

CREATE TRIGGER boxes_refresh_counters
    AFTER INSERT OR UPDATE OR DELETE ON boxes
    FOR EACH ROW EXECUTE FUNCTION tg_image_child_refresh();

CREATE TRIGGER captions_refresh_counters
    AFTER INSERT OR UPDATE OR DELETE ON captions
    FOR EACH ROW EXECUTE FUNCTION tg_image_child_refresh();
