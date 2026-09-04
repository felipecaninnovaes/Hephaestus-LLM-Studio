"""Mock CPU do motor de Difusão — responde health e devolve métricas falsas (compose.integ)."""

import json


def main() -> None:
    print(json.dumps({"status": "ok", "engine": "trainer-difusao", "mode": "mock"}))


if __name__ == "__main__":
    main()
