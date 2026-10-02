-- Migration 0023: Adiciona coluna gpu_devices na tabela orchestrators para suporte a multi-GPU (B1)
ALTER TABLE orchestrators ADD COLUMN IF NOT EXISTS gpu_devices JSONB NULL;
