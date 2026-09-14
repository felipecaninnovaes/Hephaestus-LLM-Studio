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

    def test_flux_quant_cache_validation(self):
        from trainer_difusao.models.flux import _is_cache_valid, _save_quant_metadata

        with tempfile.TemporaryDirectory() as tmpdir:
            base_dir = Path(tmpdir)
            comp_dir = base_dir / "transformer"

            # Inexistente
            self.assertFalse(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "4bit"))

            # Pasta existe, mas sem config.json nem metadata.json
            comp_dir.mkdir(parents=True, exist_ok=True)
            self.assertFalse(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "4bit"))

            # Com config.json, mas sem metadata.json (formato legado/órfão)
            (comp_dir / "config.json").write_text("{}", encoding="utf-8")
            self.assertFalse(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "4bit"))

            # Salva metadata para outro modelo (ex: unsloth)
            _save_quant_metadata(
                base_dir,
                model_id="unsloth/FLUX.2-klein-4B",
                quant_label="4-bit NF4",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
                is_flux2=True,
            )
            # Rejeita porque model_id difere
            self.assertFalse(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "4bit"))

            # Atualiza metadata para o modelo correto
            _save_quant_metadata(
                base_dir,
                model_id="black-forest-labs/FLUX.2-klein-base-4B",
                quant_label="4-bit NF4",
                quant_format="4bit",
                target_dtype="torch.bfloat16",
                is_flux2=True,
            )
            # Agora é válido!
            self.assertTrue(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "4bit"))
            # Se a quantização requisitada for 8bit, rejeita
            self.assertFalse(_is_cache_valid(comp_dir, "black-forest-labs/FLUX.2-klein-base-4B", "8bit"))


if __name__ == "__main__":
    unittest.main()

