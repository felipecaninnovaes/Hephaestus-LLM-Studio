-- Migration 0024: Adiciona coluna gpu_device na tabela jobs para suporte a seleção multi-GPU (B2)
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS gpu_device TEXT NULL;
