-- Fatia 2 / ADR-0001. Compatível com backend.md §10.
-- gen_random_uuid() é NATIVO desde o PG13 (postgres:16) — NÃO requer pgcrypto.
CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Adição ao §10 (ADR-0001 T1): segredo HS256 persistido no Postgres, linha única.
CREATE TABLE auth_state (
    id         SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    jwt_secret BYTEA NOT NULL CHECK (octet_length(jwt_secret) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
