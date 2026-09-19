import tempfile
import unittest
from pathlib import Path

try:
    import torch
    from diffusers import FluxTransformer2DModel, UNet2DConditionModel
    from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
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
    def test_qlora_diffusers_avoids_prepare_kbit_and_preserves_dtypes(self):
        """Valida que o setup canônico de LoRA em modelos diffusers (Flux, SD 1.5, SDXL)
        NÃO utiliza prepare_model_for_kbit_training do PEFT:
        1. prepare_model_for_kbit_training quebra diffusers convertendo norm_q/norm_k
           para float32 enquanto value é bfloat16, gerando erro de SDPA.
        2. O padrão correto (requires_grad_(False) + enable_gradient_checkpointing()
           + get_peft_model) preserva os dtypes homogêneos e completa forward/backward.
        """
        # 1. Demonstração do efeito colateral de prepare_model_for_kbit_training
        flux_model = FluxTransformer2DModel(
            num_layers=1,
            num_single_layers=1,
            attention_head_dim=128,
            num_attention_heads=2,
            in_channels=4,
            guidance_embeds=True,
            pooled_projection_dim=32,
            joint_attention_dim=32,
        ).to(dtype=torch.bfloat16)

        # Sem prepare: norm_q e norm_k são bfloat16
        self.assertEqual(
            flux_model.transformer_blocks[0].attn.norm_q.weight.dtype,
            torch.bfloat16,
        )
        # prepare_model_for_kbit_training converte norm_q indevidamente para float32
        corrupted = prepare_model_for_kbit_training(
            flux_model, use_gradient_checkpointing=False
        )
        self.assertEqual(
            corrupted.transformer_blocks[0].attn.norm_q.weight.dtype,
            torch.float32,
        )

        # 2. Padrão canônico Hephaestus: sem prepare_model_for_kbit_training
        clean_flux = FluxTransformer2DModel(
            num_layers=1,
            num_single_layers=1,
            attention_head_dim=128,
            num_attention_heads=2,
            in_channels=4,
            guidance_embeds=True,
            pooled_projection_dim=32,
            joint_attention_dim=32,
        ).to(dtype=torch.bfloat16)

        clean_flux.requires_grad_(False)
        clean_flux.enable_gradient_checkpointing()
        self.assertTrue(clean_flux.is_gradient_checkpointing)

        lora_config = LoraConfig(
            r=4,
            lora_alpha=4,
            init_lora_weights="gaussian",
            target_modules=["to_k", "to_q", "to_v"],
        )
        flux_peft = get_peft_model(clean_flux, lora_config)
        flux_peft.train()

        # Dtypes permanecem estritamente homogêneos em bfloat16
        self.assertEqual(
            clean_flux.transformer_blocks[0].attn.norm_q.weight.dtype,
            torch.bfloat16,
        )
        self.assertEqual(
            clean_flux.transformer_blocks[0].attn.norm_k.weight.dtype,
            torch.bfloat16,
        )

        # Executa forward e backward confirmando ausência de crash de SDPA
        hidden_states = torch.randn(1, 16, 4, dtype=torch.bfloat16)
        encoder_hidden_states = torch.randn(1, 8, 32, dtype=torch.bfloat16)
        pooled_projections = torch.randn(1, 32, dtype=torch.bfloat16)
        timestep = torch.tensor([1.0], dtype=torch.bfloat16)
        img_ids = torch.zeros(16, 3, dtype=torch.bfloat16)
        txt_ids = torch.zeros(8, 3, dtype=torch.bfloat16)
        guidance = torch.tensor([1.0], dtype=torch.bfloat16)

        out = flux_peft(
            hidden_states=hidden_states,
            encoder_hidden_states=encoder_hidden_states,
            pooled_projections=pooled_projections,
            timestep=timestep,
            img_ids=img_ids,
            txt_ids=txt_ids,
            guidance=guidance,
            return_dict=False,
        )[0]
        self.assertEqual(out.dtype, torch.bfloat16)

        loss = out.sum()
        loss.backward()
        grads = [p.grad for p in flux_peft.parameters() if p.requires_grad]
        self.assertGreater(len(grads), 0)
        self.assertTrue(all(g is not None for g in grads))

        # 3. UNet2DConditionModel (SD 1.5 / SDXL)
        unet = UNet2DConditionModel(
            sample_size=16,
            in_channels=4,
            out_channels=4,
            layers_per_block=1,
            block_out_channels=(32, 64),
            down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
            up_block_types=("CrossAttnUpBlock2D", "UpBlock2D"),
            cross_attention_dim=16,
        ).to(dtype=torch.float16)

        unet.requires_grad_(False)
        unet.enable_gradient_checkpointing()
        self.assertTrue(unet.is_gradient_checkpointing)

        unet_peft = get_peft_model(unet, lora_config)
        unet_peft.train()

        x = torch.randn(1, 4, 16, 16, dtype=torch.float16)
        t = torch.tensor([1.0], dtype=torch.float16)
        ctx = torch.randn(1, 4, 16, dtype=torch.float16)

        sample = unet_peft(x, t, encoder_hidden_states=ctx).sample
        self.assertEqual(sample.dtype, torch.float16)

        sample_loss = sample.sum()
        sample_loss.backward()
        unet_grads = [p.grad for p in unet_peft.parameters() if p.requires_grad]
        self.assertGreater(len(unet_grads), 0)
        self.assertTrue(all(g is not None for g in unet_grads))
if __name__ == "__main__":
    unittest.main()

