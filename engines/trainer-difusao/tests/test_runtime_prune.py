"""Regressão: _prune_checkpoints deve remover o par safetensors+_optimizer.pt
juntos, sem deixar .pt órfão nem apagar o .pt de um checkpoint mantido."""

import tempfile
import unittest
from pathlib import Path

from trainer_difusao.common_pkg.runtime import _prune_checkpoints


class TestPruneCheckpointsOptimizerSibling(unittest.TestCase):
    def _make_pair(self, ckpt_dir: Path, base: str, epoch: int) -> tuple[Path, Path]:
        safet = ckpt_dir / f"{base}_epoch_{epoch:03d}.safetensors"
        opt = ckpt_dir / f"{base}_epoch_{epoch:03d}_optimizer.pt"
        safet.write_bytes(b"fake-safetensors")
        opt.write_bytes(b"fake-optimizer-state")
        return safet, opt

    def test_prune_removes_optimizer_pt_sibling_of_removed_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            ckpt_dir = Path(tmp)
            pairs = [self._make_pair(ckpt_dir, "adapter", e) for e in (1, 2, 3, 4)]

            _prune_checkpoints(ckpt_dir, keep_last_n=2)

            # Épocas 1 e 2 (mais antigas) devem ter sido removidas, safetensors E .pt.
            for safet, opt in pairs[:2]:
                self.assertFalse(safet.exists(), f"{safet} deveria ter sido removido")
                self.assertFalse(opt.exists(), f".pt órfão deixado para trás: {opt}")

            # Épocas 3 e 4 (últimas keep_last_n=2) devem permanecer intactas, com .pt.
            for safet, opt in pairs[2:]:
                self.assertTrue(safet.exists())
                self.assertTrue(opt.exists(), f".pt de checkpoint mantido foi apagado: {opt}")

    def test_prune_preserves_keep_files_and_their_pt(self):
        with tempfile.TemporaryDirectory() as tmp:
            ckpt_dir = Path(tmp)
            best_safet = ckpt_dir / "best.safetensors"
            best_safet.write_bytes(b"fake-best")
            # best.safetensors não segue o padrão _epoch_NNN, então não tem .pt
            # irmão por convenção; garante apenas que não é removido no prune.
            pairs = [self._make_pair(ckpt_dir, "adapter", e) for e in (1, 2, 3)]

            _prune_checkpoints(ckpt_dir, keep_last_n=1)

            self.assertTrue(best_safet.exists())
            # Apenas a época mais recente (3) sobrevive com keep_last_n=1.
            self.assertFalse(pairs[0][0].exists())
            self.assertFalse(pairs[0][1].exists())
            self.assertFalse(pairs[1][0].exists())
            self.assertFalse(pairs[1][1].exists())
            self.assertTrue(pairs[2][0].exists())
            self.assertTrue(pairs[2][1].exists())


if __name__ == "__main__":
    unittest.main()
