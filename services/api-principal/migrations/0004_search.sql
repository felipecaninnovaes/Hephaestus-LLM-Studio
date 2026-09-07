CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE image_embeddings (
    image_id    UUID PRIMARY KEY REFERENCES images(id) ON DELETE CASCADE,
    dataset_id  UUID NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    model       TEXT NOT NULL CHECK (model IN ('ViT-B-32')),   -- v1: enum fechado, cresce por fatia
    embedding   vector(512) NOT NULL,                          -- dim fixa exigida pelo HNSW
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- dataset_id denormalizado de propósito: filtro da busca + CASCADE duplo
-- (consistência é obrigação do indexador, não há CHECK cross-tabela).
CREATE INDEX image_embeddings_dataset_model_idx ON image_embeddings (dataset_id, model);
CREATE INDEX image_embeddings_embedding_hnsw ON image_embeddings
    USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);
