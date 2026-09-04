"""Mock CPU do motor YOLO — responde health e devolve métricas falsas (compose.integ)."""

import json


def main() -> None:
    print(json.dumps({"status": "ok", "engine": "trainer-yolo", "mode": "mock"}))


if __name__ == "__main__":
    main()
