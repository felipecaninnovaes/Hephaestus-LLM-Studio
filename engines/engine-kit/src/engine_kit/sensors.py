"""
Sensores de recursos de sistema em nível de processo/container (ADR-0021 / Fatia 3a).

Coleta métricas de CPU, RAM e disco do processo do treino e I/O do container/processo
a partir de /proc sem dependências externas (sem pynvml, sem nvidia-smi, sem psutil).
Falhas de coleta retornam None de forma segura e nunca derrubam o treino.
"""
from __future__ import annotations

import math
import os
import sys
import time
from typing import Any, Optional


class SystemSensors:
    """Coletor incremental de métricas de sistema do processo."""

    def __init__(self, pid: Optional[int] = None) -> None:
        self.pid = pid or os.getpid()
        self._proc_stat_path = f"/proc/{self.pid}/stat"
        self._proc_status_path = f"/proc/{self.pid}/status"
        self._proc_io_path = f"/proc/{self.pid}/io"

        try:
            self._clk_tck = float(os.sysconf(os.sysconf_names["SC_CLK_TCK"]))
        except Exception:
            self._clk_tck = 100.0

        self._cpu_count = float(os.cpu_count() or 1)

        # Estado para cálculo de deltas
        self._last_time: Optional[float] = None
        self._last_cpu_ticks: Optional[float] = None
        self._last_read_bytes: Optional[int] = None
        self._last_write_bytes: Optional[int] = None

    def _read_cpu_ticks(self) -> Optional[float]:
        """Lê utime + stime (em clock ticks) do processo a partir de /proc/[pid]/stat."""
        try:
            with open(self._proc_stat_path, "r", encoding="utf-8") as f:
                content = f.read()
            idx = content.rfind(")")
            if idx == -1:
                return None
            fields = content[idx + 1 :].split()
            # utime é campo 14 (índice 11), stime é campo 15 (índice 12)
            utime = int(fields[11])
            stime = int(fields[12])
            return float(utime + stime)
        except Exception:
            return None

    def _read_ram_used_gb(self) -> Optional[float]:
        """Lê VmRSS do processo a partir de /proc/[pid]/status e converte para GB."""
        try:
            with open(self._proc_status_path, "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        parts = line.split()
                        if len(parts) >= 2:
                            val_kb = float(parts[1])
                            # 1 GB = 1024 * 1024 kB (1048576 kB)
                            return round(val_kb / (1024.0 * 1024.0), 3)
            return None
        except Exception:
            return None

    def _read_io_bytes(self) -> tuple[Optional[int], Optional[int]]:
        """Lê read_bytes e write_bytes a partir de /proc/[pid]/io."""
        try:
            r_bytes: Optional[int] = None
            w_bytes: Optional[int] = None
            with open(self._proc_io_path, "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("read_bytes:"):
                        r_bytes = int(line.split()[1])
                    elif line.startswith("write_bytes:"):
                        w_bytes = int(line.split()[1])
            return r_bytes, w_bytes
        except Exception:
            return None, None
    def _safe_read_cpu_ticks(self) -> Optional[float]:
        try:
            return self._read_cpu_ticks()
        except Exception:
            return None

    def _safe_read_ram_used_gb(self) -> Optional[float]:
        try:
            return self._read_ram_used_gb()
        except Exception:
            return None

    def _safe_read_io_bytes(self) -> tuple[Optional[int], Optional[int]]:
        try:
            return self._read_io_bytes()
        except Exception:
            return None, None


    def collect(self) -> Optional[dict[str, Optional[float]]]:
        """Coleta o snapshot atual de systemMetrics.

        Retorna dicionário no formato camelCase:
        {
            "cpuPct": float | None,
            "ramUsedGb": float | None,
            "diskReadMbS": float | None,
            "diskWriteMbS": float | None,
        }
        ou None caso ocorra erro irrecuperável.
        Na primeira coleta, taxas de variação são reportadas como 0.0 (baseline inicial estabelecido).
        """
        try:
            now = time.monotonic()
            cpu_ticks = self._safe_read_cpu_ticks()
            ram_used_gb = self._safe_read_ram_used_gb()
            read_bytes, write_bytes = self._safe_read_io_bytes()

            cpu_pct: Optional[float] = None
            disk_read_mb_s: Optional[float] = None
            disk_write_mb_s: Optional[float] = None

            if self._last_time is not None:
                dt = now - self._last_time
                if dt > 0:
                    # CPU % normalizado pelo número de cores (0-100%)
                    if cpu_ticks is not None and self._last_cpu_ticks is not None:
                        delta_ticks = cpu_ticks - self._last_cpu_ticks
                        if delta_ticks >= 0:
                            cpu_sec = delta_ticks / self._clk_tck
                            pct = (cpu_sec / (dt * self._cpu_count)) * 100.0
                            cpu_pct = round(max(0.0, min(100.0, pct)), 2)

                    # Taxa de disco em MB/s (1 MB = 1024 * 1024 bytes)
                    if read_bytes is not None and self._last_read_bytes is not None:
                        delta_r = read_bytes - self._last_read_bytes
                        if delta_r >= 0:
                            disk_read_mb_s = round((delta_r / (1024.0 * 1024.0)) / dt, 3)

                    if write_bytes is not None and self._last_write_bytes is not None:
                        delta_w = write_bytes - self._last_write_bytes
                        if delta_w >= 0:
                            disk_write_mb_s = round((delta_w / (1024.0 * 1024.0)) / dt, 3)
            else:
                # Primeira coleta: baseline estabelecido; taxas delta reportadas como 0.0
                if cpu_ticks is not None:
                    cpu_pct = 0.0
                if read_bytes is not None:
                    disk_read_mb_s = 0.0
                if write_bytes is not None:
                    disk_write_mb_s = 0.0

            # Atualiza baseline
            self._last_time = now
            if cpu_ticks is not None:
                self._last_cpu_ticks = cpu_ticks
            if read_bytes is not None:
                self._last_read_bytes = read_bytes
            if write_bytes is not None:
                self._last_write_bytes = write_bytes

            return {
                "cpuPct": cpu_pct,
                "ramUsedGb": ram_used_gb,
                "diskReadMbS": disk_read_mb_s,
                "diskWriteMbS": disk_write_mb_s,
            }
        except Exception as exc:
            print(f"[WARN] Falha ao coletar sensores do sistema: {exc}", file=sys.stderr, flush=True)
            return None


def sanitize_system_metrics(metrics: Any) -> Optional[dict[str, Any]]:
    """Sanitiza e valida payload de system_metrics garantindo as 4 chaves e JSON estrito."""
    if not isinstance(metrics, dict):
        return None

    res: dict[str, Any] = {}
    key_map = {
        "cpu_pct": "cpuPct",
        "cpuPct": "cpuPct",
        "ram_used_gb": "ramUsedGb",
        "ramUsedGb": "ramUsedGb",
        "disk_read_mb_s": "diskReadMbS",
        "diskReadMbS": "diskReadMbS",
        "disk_write_mb_s": "diskWriteMbS",
        "diskWriteMbS": "diskWriteMbS",
    }

    for src_k, target_k in key_map.items():
        if src_k in metrics:
            val = metrics[src_k]
            if val is None:
                res[target_k] = None
            else:
                try:
                    f_val = float(val)
                    if math.isfinite(f_val):
                        res[target_k] = max(0.0, f_val)
                    else:
                        res[target_k] = None
                except (ValueError, TypeError):
                    res[target_k] = None

    # Assegura presença das 4 chaves obrigatórias do contrato
    for k in ("cpuPct", "ramUsedGb", "diskReadMbS", "diskWriteMbS"):
        if k not in res:
            res[k] = None

    return res


# Instância singleton global para reuso por padrão quando desejado
_GLOBAL_SENSORS: Optional[SystemSensors] = None


def get_global_sensors() -> SystemSensors:
    """Retorna ou inicializa o singleton de sensores do processo."""
    global _GLOBAL_SENSORS
    if _GLOBAL_SENSORS is None:
        _GLOBAL_SENSORS = SystemSensors()
    return _GLOBAL_SENSORS


def collect_system_metrics() -> Optional[dict[str, Optional[float]]]:
    """Coleta métricas usando o coletor global."""
    return get_global_sensors().collect()
