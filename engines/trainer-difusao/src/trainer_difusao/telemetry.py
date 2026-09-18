"""
Módulo de Telemetria Padronizada para Engines do Hephaestus Studio (ADR-0021).

Re-exporta a implementação canônica de hephaestus-engine-kit.
"""
from engine_kit.telemetry import TelemetryEmitter

__all__ = ["TelemetryEmitter"]
