"""trainer-difusao — motor de treino LoRA de Difusão (mock/real).

Entrypoint para CLI e health check. Delega para trainer_difusao.train.
"""

from __future__ import annotations

from trainer_difusao.train import main

if __name__ == "__main__":
    main()
