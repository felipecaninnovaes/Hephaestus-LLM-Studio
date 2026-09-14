import tempfile
import unittest
from pathlib import Path

from trainer_difusao.models import (
    BaseModelTrainer,
    FluxTrainer,
    MockTrainer,
    SD15Trainer,
    SDXLTrainer,
    get_trainer,
)
from trainer_difusao.optimizers import _create_optimizer


class TestModelsDecoupling(unittest.TestCase):
    def test_get_trainer_factory(self):
        # Modo mock
        mock_trainer = get_trainer("flux", is_mock=True)
        self.assertIsInstance(mock_trainer, MockTrainer)
        self.assertIsInstance(mock_trainer, BaseModelTrainer)

        # Modo real por modelo canônico
        flux_trainer = get_trainer("flux-2-klein-4b", is_mock=False)
        self.assertIsInstance(flux_trainer, FluxTrainer)

        flux_alias = get_trainer("flux", is_mock=False)
        self.assertIsInstance(flux_alias, FluxTrainer)

        sdxl_trainer = get_trainer("sdxl", is_mock=False)
        self.assertIsInstance(sdxl_trainer, SDXLTrainer)

        sd15_trainer = get_trainer("sd15", is_mock=False)
        self.assertIsInstance(sd15_trainer, SD15Trainer)

    def test_get_trainer_unknown_raises(self):
        with self.assertRaises(SystemExit):
            get_trainer("modelo_inexistente_xyz", is_mock=False)

    def test_optimizer_requires_trainable_params(self):
        class DummyFrozenModule:
            def parameters(self):
                class Param:
                    requires_grad = False

                return [Param(), Param()]

        dummy = DummyFrozenModule()
        with self.assertRaises(ValueError) as ctx:
            _create_optimizer(dummy, "adamw", lr=1e-4)
        self.assertIn("Nenhum parâmetro com requires_grad=True", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
