"""Regressao: remap de keys legado ESRGAN -> BasicSR (ultrasharp/siax).

Pesos ultrasharp/siax usam o layout legado ("model.0...", "model.1.sub...").
A RRDBNet vendida usa o naming BasicSR. CPU, rapido, sem rede/pesos reais.
"""

import re
import unittest

_BASICS_R_HEAD = {
    "conv_first": "model.0",
    "conv_body": "model.1.sub.23",
    "conv_up1": "model.3",
    "conv_up2": "model.6",
    "conv_hr": "model.8",
    "conv_last": "model.10",
}
_HEAD_RE = re.compile(r"^(conv_first|conv_body|conv_up1|conv_up2|conv_hr|conv_last)\.(.+)$")
_BODY_RE = re.compile(r"^body\.(\d+)\.rdb([123])\.conv([1-5])\.(.+)$")


def _basicsr_to_legacy(key: str) -> str:
    """Inverso do remap: gera key legada a partir da key BasicSR vendida."""
    m = _HEAD_RE.match(key)
    if m:
        return f"{_BASICS_R_HEAD[m.group(1)]}.{m.group(2)}"
    m = _BODY_RE.match(key)
    if m:
        idx, rdb, conv, tail = m.groups()
        return f"model.1.sub.{int(idx)}.RDB{rdb}.conv{conv}.0.{tail}"
    raise AssertionError(f"key BasicSR inesperada no teste: {key}")
try:
    import torch
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False


@unittest.skipUnless(HAS_TORCH, "requer torch instalado")
class TestRemapLegacyRoundTrip(unittest.TestCase):
    """state_dict legado sintetico (shapes exatos) -> remap -> strict load + forward."""

    def test_legacy_remap_loads_strict_and_forwards(self):
        import torch

        from trainer_difusao.upscale import _build_rrdb_net, remap_esrgan_keys

        src = _build_rrdb_net()
        sd_new = src.state_dict()
        legacy = {_basicsr_to_legacy(k): v for k, v in sd_new.items()}
        self.assertTrue(any(k.startswith("model.") for k in legacy))

        remapped = remap_esrgan_keys(legacy)
        self.assertEqual(set(remapped.keys()), set(sd_new.keys()))

        net = _build_rrdb_net()
        net.load_state_dict(remapped, strict=True)
        net.eval()
        with torch.inference_mode():
            out = net(torch.randn(1, 3, 16, 16))
        self.assertEqual(tuple(out.shape), (1, 3, 64, 64))

    def test_new_format_untouched(self):
        from trainer_difusao.upscale import _build_rrdb_net, remap_esrgan_keys

        sd_new = _build_rrdb_net().state_dict()
        self.assertIs(remap_esrgan_keys(sd_new), sd_new)

    def test_unmapped_legacy_key_dies(self):
        import torch

        from trainer_difusao.upscale import remap_esrgan_keys

        with self.assertRaises(SystemExit):
            remap_esrgan_keys({"model.99.weight": torch.zeros(1)})


if __name__ == "__main__":
    unittest.main()
