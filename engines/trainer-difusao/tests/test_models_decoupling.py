import tempfile
import unittest
from pathlib import Path

try:
    import torch
    from diffusers import FluxTransformer2DModel, UNet2DConditionModel
    from peft import prepare_model_for_kbit_training
    HAS_DIFFUSERS_PEFT = True
except ImportError:
    HAS_DIFFUSERS_PEFT = False
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

    @unittest.skipUnless(HAS_DIFFUSERS_PEFT, "requer torch, diffusers e peft instalados")
    def test_qlora_prepare_diffusers_models_without_get_input_embeddings(self):
        """Garante que modelos diffusers quantizados (Flux, SD 1.5, SDXL) funcionem
        com prepare_model_for_kbit_training(use_gradient_checkpointing=False)
        seguido de enable_gradient_checkpointing(), evitando o crash de
        get_input_embeddings ausente em ModelMixin do diffusers.
        """
        # 1. FluxTransformer2DModel
        flux_model = FluxTransformer2DModel(
            num_layers=1,
            num_single_layers=1,
            attention_head_dim=16,
            num_attention_heads=2,
            in_channels=4,
        )
        flux_model.is_loaded_in_4bit = True
        self.assertFalse(hasattr(flux_model, "get_input_embeddings"))

        # Valida que use_gradient_checkpointing=True reproduz o bug
        with self.assertRaises(AttributeError) as ctx:
            prepare_model_for_kbit_training(flux_model, use_gradient_checkpointing=True)
        self.assertIn("get_input_embeddings", str(ctx.exception))

        # Valida correção arquitetural
        prepared = prepare_model_for_kbit_training(flux_model, use_gradient_checkpointing=False)
        prepared.enable_gradient_checkpointing()
        self.assertTrue(prepared.is_gradient_checkpointing)

        # 2. UNet2DConditionModel (SD 1.5 / SDXL)
        unet = UNet2DConditionModel(
            sample_size=16,
            in_channels=4,
            out_channels=4,
            layers_per_block=1,
            block_out_channels=(32, 64),
            down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
            up_block_types=("CrossAttnUpBlock2D", "UpBlock2D"),
            cross_attention_dim=16,
        )
        unet.is_loaded_in_4bit = True
        self.assertFalse(hasattr(unet, "get_input_embeddings"))

        with self.assertRaises(AttributeError) as ctx:
            prepare_model_for_kbit_training(unet, use_gradient_checkpointing=True)
        self.assertIn("get_input_embeddings", str(ctx.exception))

        prepared_unet = prepare_model_for_kbit_training(unet, use_gradient_checkpointing=False)
        prepared_unet.enable_gradient_checkpointing()
        self.assertTrue(prepared_unet.is_gradient_checkpointing)

if __name__ == "__main__":
    unittest.main()

