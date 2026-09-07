"""trainer-yolo — motor de treino YOLO (mock/real).

Entrypoint para CLI e health check. Delega para trainer_yolo.train.
"""

from __future__ import annotations

from trainer_yolo.train import main

if __name__ == "__main__":
    main()
