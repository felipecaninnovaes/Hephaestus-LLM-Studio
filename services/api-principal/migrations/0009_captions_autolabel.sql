-- Migration 0009: aceita origin 'autolabel' na tabela captions (ADR-0016 D2).
-- Permite que jobs de AutoLabel retornem captions com origem honesta 'autolabel'.

ALTER TABLE captions DROP CONSTRAINT IF EXISTS captions_origin_check;
ALTER TABLE captions ADD CONSTRAINT captions_origin_check
    CHECK (origin IN ('manual', 'autolabel', 'autotracker', 'import'));
