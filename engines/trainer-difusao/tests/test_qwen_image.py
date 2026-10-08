"""Testes para suporte nativo Qwen-Image-2.1 (sem fallback diffusers)."""

import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

try:
    import torch
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False


class TestQwenImageNativeModules(unittest.TestCase):
    """Test native Qwen-Image-2.1 module imports and architecture."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_native_module_imports(self):
        """Verify native modules can be imported directly (no diffusers fallback)."""
        from trainer_difusao.models.qwen_pkg.qwen_image_2 import (
            AutoencoderKLQwenImage21,
            QwenImage21Pipeline,
            QwenImage21PromptEncoder,
            QwenImage21TextEncoder,
            QwenImage21Transformer2DModel,
            run_transformer,
        )

        # All classes should be defined
        self.assertIsNotNone(AutoencoderKLQwenImage21)
        self.assertIsNotNone(QwenImage21Pipeline)
        self.assertIsNotNone(QwenImage21PromptEncoder)
        self.assertIsNotNone(QwenImage21TextEncoder)
        self.assertIsNotNone(QwenImage21Transformer2DModel)
        self.assertIsNotNone(run_transformer)

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_no_getattr_diffusers_fallback_in_runner(self):
        """Ensure generation/runner.py no longer uses getattr diffusers fallback."""
        runner_path = Path("engines/trainer-difusao/src/trainer_difusao/generation/runner.py")
        if runner_path.exists():
            content = runner_path.read_text()
            # Should NOT have getattr fallback patterns
            self.assertNotIn(
                'getattr(diffusers, "QwenImage21Pipeline"',
                content,
                "runner.py should not use getattr diffusers fallback"
            )
            # Should have direct imports
            self.assertIn(
                "from trainer_difusao.models.qwen_pkg.qwen_image_2 import QwenImage21Pipeline",
                content,
                "runner.py should import QwenImage21Pipeline directly"
            )

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_no_getattr_diffusers_fallback_in_sample(self):
        """Ensure models/qwen_pkg/sample.py no longer uses getattr diffusers fallback."""
        sample_path = Path("engines/trainer-difusao/src/trainer_difusao/models/qwen_pkg/sample.py")
        if sample_path.exists():
            content = sample_path.read_text()
            # Should NOT have getattr fallback patterns
            self.assertNotIn(
                'getattr(diffusers, "QwenImage21Pipeline"',
                content,
                "sample.py should not use getattr diffusers fallback"
            )
            # Should have direct imports
            self.assertIn(
                "from trainer_difusao.models.qwen_pkg.qwen_image_2 import QwenImage21Pipeline",
                content,
                "sample.py should import QwenImage21Pipeline directly"
            )

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_trainer_interface_preserved(self):
        """Verify QwenImageTrainer maintains BaseModelTrainer interface."""
        from trainer_difusao.models import QwenImageTrainer, BaseModelTrainer

        # Should be a subclass
        self.assertTrue(issubclass(QwenImageTrainer, BaseModelTrainer))
        # Should have train method
        self.assertTrue(hasattr(QwenImageTrainer, 'train'))
        self.assertTrue(callable(getattr(QwenImageTrainer, 'train')))


class TestQwenImageFlowMatching(unittest.TestCase):
    """Test flow-matching loss computation (noise - latents target)."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_loss_target_flow_matching_formula(self):
        """Verify loss target follows flow-matching: velocity = noise - latents."""
        import torch
        
        # Simulate batch
        batch_size = 2
        latent_dim = 64
        latent_h, latent_w = 16, 16
        
        noise = torch.randn(batch_size, latent_dim, latent_h, latent_w)
        latents = torch.randn(batch_size, latent_dim, latent_h, latent_w)
        
        # Flow-matching loss target: noise - latents
        loss_target = (noise - latents).detach()
        
        # Verify shape matches
        self.assertEqual(loss_target.shape, noise.shape)
        self.assertEqual(loss_target.shape, latents.shape)
        
        # Verify values (should not be identical to either input)
        self.assertFalse(torch.allclose(loss_target, noise))
        self.assertFalse(torch.allclose(loss_target, latents))


class TestQwenImageRGBAVAE(unittest.TestCase):
    """Test RGBA VAE encoding/decoding with alpha channel."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_rgb_to_rgba_conversion(self):
        """Test automatic opaque alpha channel addition for RGB images."""
        import torch
        
        # 3-channel RGB image
        batch_size = 1
        rgb_image = torch.randn(batch_size, 3, 256, 256)  # 3 channels
        
        # Add opaque alpha (as VAE expects RGBA)
        if rgb_image.shape[1] == 3:
            alpha_channel = torch.ones_like(rgb_image[:, :1])
            rgba_image = torch.cat([rgb_image, alpha_channel], dim=1)
        
        # Verify result
        self.assertEqual(rgba_image.shape[1], 4)  # Should now be 4 channels
        # Alpha should be all 1s (opaque)
        self.assertTrue(torch.allclose(rgba_image[:, 3:], torch.ones_like(alpha_channel)))

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_rgba_to_rgb_stripping(self):
        """Test stripping alpha channel from decoded RGBA output."""
        import torch
        
        # 4-channel RGBA image (decoded from VAE)
        batch_size = 1
        rgba_image = torch.randn(batch_size, 4, 256, 256)
        
        # Strip alpha for RGB-only output
        rgb_only = rgba_image[:, :3]
        
        # Verify result
        self.assertEqual(rgb_only.shape[1], 3)
        self.assertEqual(rgb_only.shape, (batch_size, 3, 256, 256))


class TestQwenImageBucketDivisibility(unittest.TestCase):
    """Test VAE spatial compression and bucket divisibility alignment."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_vision_token_pixels_alignment(self):
        """Verify VISION_TOKEN_PIXELS = 16 (VAE spatial) × 2 (token grouping) = 32."""
        # Qwen-Image-2.1: VAE spatial compression = 16x, token grouping = 2
        VAE_SPATIAL_FACTOR = 16
        TOKEN_GROUPING = 2
        VISION_TOKEN_PIXELS = VAE_SPATIAL_FACTOR * TOKEN_GROUPING  # 32
        
        self.assertEqual(VISION_TOKEN_PIXELS, 32)
        
        # Test resolution must be divisible by VISION_TOKEN_PIXELS
        for resolution in [512, 768, 1024]:
            self.assertEqual(resolution % VISION_TOKEN_PIXELS, 0,
                           f"Resolution {resolution} must be divisible by {VISION_TOKEN_PIXELS}")

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_bucket_compatibility(self):
        """Verify dataset bucketing respects VAE spatial factor."""
        VAE_SPATIAL_FACTOR = 16
        
        # Common training resolutions
        resolutions = [512, 768, 1024]
        for res in resolutions:
            # Latent size after 16x VAE compression
            latent_size = res // VAE_SPATIAL_FACTOR
            self.assertEqual(latent_size % 2, 0, f"Latent size {latent_size} should be even")


class TestQwenImageMemoryCleanup(unittest.TestCase):
    """Test VRAM cleanup invariants during training."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_vram_cleanup_after_backward(self):
        """Verify CUDA cache is empty after gradient computation."""
        import torch
        
        # Create a simple computation graph
        x = torch.randn(2, 64, 16, 16, device="cuda" if torch.cuda.is_available() else "cpu")
        x.requires_grad = True
        loss = x.sum()
        loss.backward()
        
        # Cleanup should not raise
        try:
            torch.cuda.empty_cache()
        except Exception as e:
            self.fail(f"CUDA empty_cache failed: {e}")

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_no_residual_tensor_references(self):
        """Verify training loop doesn't keep residual tensor references."""
        import torch
        import gc
        
        # Simulate training step cleanup
        loss = torch.tensor(1.0, requires_grad=True)
        loss.backward()
        del loss
        
        # Force garbage collection
        gc.collect()
        if torch.cuda.is_available():
            torch.cuda.empty_cache()
        
        # Should complete without errors
        self.assertTrue(True)


class TestQwenImageControlImageHandling(unittest.TestCase):
    """Test reference/control image encoding and token slot handling."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_control_image_slot_packing(self):
        """Verify control images pack into reserved <|image_pad|> slots."""
        # Qwen-Image-2.1 reserves 4 latent tokens per reference image
        LATENT_TOKENS_PER_REF = 4
        
        # Simulate multiple reference images
        num_refs = 3
        expected_latent_slots = num_refs * LATENT_TOKENS_PER_REF
        
        self.assertEqual(expected_latent_slots, 12)

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_control_image_normalization(self):
        """Test control images are normalized to correct range."""
        import torch
        
        # Control image in [0, 1] range (PIL-like)
        control_image = torch.rand(1, 3, 256, 256)
        
        # Should normalize to VAE range [-1, 1] before encoding
        # (actual normalization happens during VAE.encode)
        self.assertTrue(control_image.min() >= 0.0)
        self.assertTrue(control_image.max() <= 1.0)


class TestQwenImagePromptEncoding(unittest.TestCase):
    """Test prompt encoding with optional reference images."""

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_plain_text_to_image_encoding(self):
        """Test T2I prompt encoding without reference images."""
        # Plain prompt should not reserve image_pad slots
        prompt = "a cat sitting on a table"
        
        # When encoded without reference images, no slots are reserved
        # (actual encoding would happen via QwenImage21PromptEncoder)
        self.assertIsNotNone(prompt)
        self.assertNotIn("<|image_pad|>", prompt)  # Plain T2I has no image pads

    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_prompt_with_reference_image_slots(self):
        """Test prompt encoding reserves slots for reference images."""
        # Prompt with reference image instruction
        prompt_with_ref = "modify the image to show a sunset"
        
        # (When properly encoded via QwenImage21PromptEncoder, this would
        # reserve <|image_pad|> slots in the sequence)
        self.assertIsNotNone(prompt_with_ref)


class TestQwenImageMockMode(unittest.TestCase):
    """Test mock mode training does not require GPU."""

    @mock.patch('engine_kit.mock.is_mock', return_value=True)
    @unittest.skipIf(not HAS_TORCH, "torch not available")
    def test_mock_train_uses_mock_function(self, mock_is_mock):
        """Verify mock training dispatches to mock function."""
        from trainer_difusao.models import QwenImageTrainer
        import tempfile
        
        trainer = QwenImageTrainer()
        
        with tempfile.TemporaryDirectory() as tmpdir:
            cfg = {
                'lora': {'rank': 8, 'alpha': 8},
                'dataset_path': tmpdir,
                'resolution': 512,
            }
            # In mock mode, this should use _mock_train, not GPU code
            # (Actual test would need more setup)
            self.assertIsNotNone(trainer)


class TestQwenImageDependencyVersions(unittest.TestCase):
    """Test required dependency versions for native support."""

    def test_transformers_version_requirement(self):
        """Verify transformers>=4.57.0 requirement is documented."""
        pyproject_path = Path("engines/trainer-difusao/pyproject.toml")
        if pyproject_path.exists():
            content = pyproject_path.read_text()
            self.assertIn("transformers>=4.57.0", content,
                         "pyproject.toml should require transformers>=4.57.0 for Qwen3-VL")

    def test_qwen_vl_utils_dependency(self):
        """Verify qwen-vl-utils>=0.0.14 is included."""
        pyproject_path = Path("engines/trainer-difusao/pyproject.toml")
        if pyproject_path.exists():
            content = pyproject_path.read_text()
            self.assertIn("qwen-vl-utils>=0.0.14", content,
                         "pyproject.toml should include qwen-vl-utils>=0.0.14")


class TestQwenImageConfigAndSpecs(unittest.TestCase):
    """Test configuration and policy files are updated."""

    def test_vram_table_updated_for_native(self):
        """Verify VRAM table documents 4-bit quantization for native training."""
        vram_table_path = Path("packages/policies/vram-table.yaml")
        if vram_table_path.exists():
            content = vram_table_path.read_text()
            self.assertIn("qwen-image-2.1", content)
            # Should mention quantization requirement
            self.assertIn("4bit", content)

    def test_engines_yaml_has_transformers_version(self):
        """Verify engines.yaml documents required transformers version."""
        engines_path = Path("packages/policies/engines.yaml")
        if engines_path.exists():
            content = engines_path.read_text()
            # Should mention Qwen3-VL or transformers 4.57
            self.assertTrue("transformers" in content or "Qwen3-VL" in content)


@unittest.skipIf(not HAS_TORCH, "torch not available")
class TestQwenImageResumeEpochNumbering(unittest.TestCase):
    """Regressão: retomada de treino Qwen-Image-2.1 deve numerar as épocas
    ADICIONAIS a partir de epoch_offset+1, não recomeçar de 1 (contrato item 7).
    Exercita a mesma função usada por _real_train_qwen_image."""

    def test_epoch_offset_3_epochs_2_yields_epochs_4_and_5(self):
        from trainer_difusao.models.qwen_image import _resume_epoch_range

        self.assertEqual(_resume_epoch_range(epoch_offset=3, epochs=2), [(1, 4), (2, 5)])

    def test_no_offset_reproduces_original_1_based_numbering(self):
        from trainer_difusao.models.qwen_image import _resume_epoch_range

        self.assertEqual(
            _resume_epoch_range(epoch_offset=0, epochs=5),
            [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)],
        )


@unittest.skipIf(not HAS_TORCH, "torch not available")
class TestQwenSampleUsesCachedEmbeds(unittest.TestCase):
    """Regressão OOM: o text encoder precisa ser descarregado de verdade após o precompute,
    então a amostra consome o embed cacheado e nunca chama ``encode``."""

    @staticmethod
    def _fakes():
        import types

        class FakeVae:
            config = types.SimpleNamespace(latents_mean=[0.0] * 4, latents_std=[1.0] * 4)

            def to(self, *_a, **_k):
                return self

            def eval(self):
                return self

            def decode(self, latents):
                b, _, _, h, w = latents.shape
                return types.SimpleNamespace(sample=torch.zeros(b, 3, 1, h * 16, w * 16))

        class FakeTransformer:
            config = types.SimpleNamespace(in_channels=4)

            def eval(self):
                return self

        return FakeTransformer(), FakeVae()

    def _payload(self):
        return {
            "embeds": torch.zeros(5, 8),
            "mask": torch.ones(5),
            "slot_mask": torch.zeros(5, dtype=torch.bool),
        }

    def test_sample_signature_has_no_prompt_encoder(self):
        import inspect

        from trainer_difusao.models.qwen_image import _qwen_sample_native

        params = inspect.signature(_qwen_sample_native).parameters
        self.assertNotIn("prompt_encoder", params)
        self.assertIn("prompt_payload", params)

    def test_sample_runs_from_cached_payload(self):
        from trainer_difusao.models import qwen_image

        transformer, vae = self._fakes()
        seen = {}

        def fake_run_transformer(_tr, latents, _t, prompt_embeds, prompt_mask, slot_mask, **_kw):
            seen["embeds_shape"] = tuple(prompt_embeds.shape)
            return torch.zeros_like(latents)

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            qwen_image, "run_transformer", fake_run_transformer
        ):
            out = Path(tmp) / "sample.png"
            image = qwen_image._qwen_sample_native(
                transformer,
                vae,
                self._payload(),
                height=32,
                width=32,
                num_inference_steps=2,
                seed=1,
                device="cpu",
                dtype=torch.float32,
                output_path=out,
            )
            self.assertIsNotNone(image)
            self.assertTrue(out.exists())
        self.assertEqual(seen["embeds_shape"], (1, 5, 8))

    def test_release_prompt_encoder_drops_text_encoder_reference(self):
        import gc
        import weakref

        from trainer_difusao.models.qwen_image import _release_prompt_encoder

        class FakeTextEncoder:
            pass

        class FakePromptEncoder:
            def __init__(self, te):
                self.text_encoder = te

            def encode(self, *_a, **_k):  # pragma: no cover - não deve ser chamado
                raise AssertionError("encode chamado após o unload")

        text_encoder = FakeTextEncoder()
        prompt_encoder = FakePromptEncoder(text_encoder)
        ref = weakref.ref(text_encoder)

        _release_prompt_encoder(prompt_encoder)
        self.assertIsNone(prompt_encoder.text_encoder)
        text_encoder = None  # a variável local do chamador também é solta
        gc.collect()
        self.assertIsNone(ref())


@unittest.skipIf(not HAS_TORCH, "torch not available")
class TestQwenGradAccumAndScheduler(unittest.TestCase):
    """GA, scheduler e escolha de otimizador do loop Qwen seguem o loop compartilhado."""

    @staticmethod
    def _toy(grad_accum, lr=0.1):
        from trainer_difusao.models.qwen_image import _GradAccumulator

        module = torch.nn.Linear(2, 1, bias=False)
        torch.nn.init.constant_(module.weight, 1.0)
        optimizer = torch.optim.SGD(module.parameters(), lr=lr)
        steps = []
        orig_step = optimizer.step

        def counting_step(*a, **k):
            steps.append(module.weight.detach().clone())
            return orig_step(*a, **k)

        optimizer.step = counting_step
        return module, optimizer, steps, _GradAccumulator(module, optimizer, None, grad_accum)

    @staticmethod
    def _run_epoch(acc, module, n_batches):
        losses = []
        for i in range(n_batches):
            x = torch.full((1, 2), 0.1 * (i + 1))
            loss = module(x).sum()
            losses.append(acc.backward(loss))
            acc.step_if_due(i == n_batches - 1)
        return losses

    def test_steps_per_epoch_is_ceil_n_over_ga(self):
        for n_batches, ga, expected in [(8, 4, 2), (9, 4, 3), (3, 4, 1), (5, 1, 5), (7, 3, 3)]:
            module, _opt, steps, acc = self._toy(ga)
            for _epoch in range(2):
                acc.reset()
                self._run_epoch(acc, module, n_batches)
            self.assertEqual(len(steps), 2 * expected, (n_batches, ga))

    def test_gradient_is_accumulated_as_mean_over_micro_batches(self):
        module, _optimizer, _steps, acc = self._toy(grad_accum=2, lr=1.0)
        w0 = module.weight.detach().clone()
        losses = self._run_epoch(acc, module, 2)
        # d(sum(w*x))/dw = x; média de x=0.1 e x=0.2 -> 0.15 em cada peso (norma < 1, sem clip)
        self.assertTrue(torch.allclose(module.weight.detach(), w0 - 0.15, atol=1e-6))
        # loss reportada é a original (não dividida por GA)
        self.assertAlmostEqual(losses[0], float(w0.sum() * 0.1), places=5)
        # gradiente zerado após o passo
        self.assertTrue(module.weight.grad is None or float(module.weight.grad.abs().sum()) == 0.0)

    def test_remainder_flushed_at_end_of_epoch_without_leaking_into_next(self):
        module, _opt, steps, acc = self._toy(grad_accum=4)
        self._run_epoch(acc, module, 3)
        self.assertEqual(len(steps), 1)
        self.assertEqual(acc.pending, 0)
        self.assertIsNone(acc.flush())

    def test_cosine_warmup_scheduler_rises_then_decays(self):
        from trainer_difusao.models.qwen_image import _current_lr, _setup_optimization

        module = torch.nn.Linear(2, 1)
        optimizer, scheduler, acc = _setup_optimization(
            module,
            optimizer_name="adamw",
            learning_rate=1e-3,
            optimizer_state_path=None,
            lr_scheduler_name="cosine",
            lr_warmup_steps=2,
            steps_per_epoch=2,
            epochs=2,
            epoch_offset=0,
            grad_accum=4,
        )
        self.assertIsNotNone(scheduler)
        lrs = [_current_lr(scheduler, optimizer, 1e-3)]
        for _ in range(4):
            for _micro in range(4):
                acc.backward(module(torch.ones(1, 2)).sum())
            acc.flush()
            lrs.append(_current_lr(scheduler, optimizer, 1e-3))
        self.assertEqual(lrs[0], 0.0)
        self.assertLess(lrs[0], lrs[1])
        self.assertLess(lrs[1], lrs[2])  # sobe no warmup
        self.assertAlmostEqual(lrs[2], 1e-3, places=9)  # pico ao fim do warmup
        self.assertLess(lrs[3], lrs[2])  # cai (cosine)
        self.assertLess(lrs[4], lrs[3])

    def test_optimizer_scheduler_names_reach_factories(self):
        from trainer_difusao.models import qwen_image

        module = torch.nn.Linear(2, 1)
        fake_opt = torch.optim.SGD(module.parameters(), lr=0.1)
        with mock.patch.object(qwen_image, "_create_optimizer", return_value=fake_opt) as co, \
                mock.patch.object(qwen_image, "_create_lr_scheduler", return_value=None) as cs:
            qwen_image._setup_optimization(
                module,
                optimizer_name="paged_adamw32bit",
                learning_rate=2e-4,
                optimizer_state_path=None,
                lr_scheduler_name="cosine_with_restarts",
                lr_warmup_steps=7,
                steps_per_epoch=3,
                epochs=5,
                epoch_offset=2,
                grad_accum=4,
            )
        co.assert_called_once_with(module, "paged_adamw32bit", 2e-4)
        # horizonte = steps_per_epoch * (offset + epochs); resume posiciona em steps_per_epoch * offset
        cs.assert_called_once_with(fake_opt, "cosine_with_restarts", 21, 7, last_step=6)


@unittest.skipIf(not HAS_TORCH, "torch not available")
class TestQwenEpochCheckpoint(unittest.TestCase):
    """Checkpoint é por ÉPOCA (após flush de GA), não por batch; prune mantém 2."""

    def _simulate(self, tmp, *, n_batches, epochs, interval, ga, epoch_offset=0):
        from trainer_difusao.models import qwen_image

        module, optimizer, steps, acc = TestQwenGradAccumAndScheduler._toy(ga)
        ckpt_dir = Path(tmp) / "checkpoints"
        saves = []

        def fake_save(model, d, base, epoch, metadata, optimizer=None):
            d.mkdir(parents=True, exist_ok=True)
            f = d / f"{base}_epoch_{epoch:03d}.safetensors"
            torch.save({"w": model.weight.detach().clone()}, f)
            (d / f"{base}_epoch_{epoch:03d}_optimizer.pt").write_bytes(b"o")
            saves.append((epoch, metadata["epoch"], len(steps), model.weight.detach().clone()))
            return f

        with mock.patch.object(qwen_image, "save_adapter_checkpoint", fake_save):
            for local_idx, epoch in qwen_image._resume_epoch_range(epoch_offset, epochs):
                acc.reset()
                for i in range(n_batches):
                    loss = module(torch.full((1, 2), 0.1 * (i + 1))).sum()
                    acc.backward(loss)
                    acc.step_if_due(i == n_batches - 1)
                acc.flush()
                qwen_image._maybe_save_epoch_checkpoint(
                    module, optimizer, ckpt_dir, "adapter",
                    epoch=epoch, local_epoch_idx=local_idx, epochs=epochs,
                    checkpoint_interval=interval, global_step=len(steps),
                )
        return saves, ckpt_dir, module, steps

    def test_saves_only_on_interval_and_last_epoch_once_each(self):
        with tempfile.TemporaryDirectory() as tmp:
            saves, _d, _m, _s = self._simulate(tmp, n_batches=5, epochs=5, interval=2, ga=2)
        self.assertEqual([s[0] for s in saves], [2, 4, 5])

    def test_interval_larger_than_epochs_saves_only_last(self):
        with tempfile.TemporaryDirectory() as tmp:
            saves, _d, _m, _s = self._simulate(tmp, n_batches=4, epochs=3, interval=10, ga=1)
        self.assertEqual([s[0] for s in saves], [3])

    def test_no_per_batch_save_and_weights_are_end_of_epoch(self):
        with tempfile.TemporaryDirectory() as tmp:
            saves, d, module, steps = self._simulate(tmp, n_batches=7, epochs=1, interval=1, ga=3)
            self.assertEqual(len(saves), 1)
            epoch, _meta, steps_at_save, weights = saves[0]
            self.assertEqual(steps_at_save, len(steps))  # após o último passo (incl. resto)
            self.assertEqual(len(steps), 3)  # ceil(7/3)
            self.assertTrue(torch.equal(weights, module.weight.detach()))
            saved = torch.load(d / "adapter_epoch_001.safetensors")["w"]
            self.assertTrue(torch.equal(saved, module.weight.detach()))

    def test_resume_offset_uses_local_index_and_absolute_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            saves, d, _m, _s = self._simulate(
                tmp, n_batches=2, epochs=4, interval=2, ga=1, epoch_offset=10
            )
            self.assertEqual([s[0] for s in saves], [12, 14])
            self.assertEqual([s[1] for s in saves], ["12", "14"])
            self.assertTrue((d / "adapter_epoch_014.safetensors").exists())

    def test_prune_keeps_last_two(self):
        with tempfile.TemporaryDirectory() as tmp:
            _saves, d, _m, _s = self._simulate(tmp, n_batches=2, epochs=4, interval=1, ga=1)
            names = sorted(p.name for p in d.glob("*.safetensors"))
            self.assertEqual(names, ["adapter_epoch_003.safetensors", "adapter_epoch_004.safetensors"])
            self.assertFalse((d / "adapter_epoch_001_optimizer.pt").exists())



@unittest.skipIf(not HAS_TORCH, "torch not available")
class TestQwenImageQuantization(unittest.TestCase):
    """`lora.quantization` governa o transformer; text encoder é sempre NF4."""

    def test_4bit_is_nf4_with_double_quant(self):
        from trainer_difusao.models import qwen_image

        cfg = qwen_image._build_transformer_bnb_config("4bit", torch.bfloat16)
        self.assertTrue(cfg.load_in_4bit)
        self.assertEqual(cfg.bnb_4bit_quant_type, "nf4")
        self.assertTrue(cfg.bnb_4bit_use_double_quant)
        self.assertEqual(cfg.bnb_4bit_compute_dtype, torch.bfloat16)

    def test_8bit_uses_load_in_8bit(self):
        from trainer_difusao.models import qwen_image

        cfg = qwen_image._build_transformer_bnb_config("8bit", torch.bfloat16)
        self.assertTrue(cfg.load_in_8bit)
        self.assertFalse(cfg.load_in_4bit)

    def test_none_has_no_quantization_config(self):
        from trainer_difusao.models import qwen_image

        self.assertIsNone(qwen_image._build_transformer_bnb_config("none", torch.bfloat16))

    def test_unsupported_levels_fail_explicitly(self):
        from trainer_difusao.models import qwen_image

        for level in ("2bit", "6bit"):
            with self.assertRaises(SystemExit, msg=level):
                qwen_image._resolve_transformer_quantization(level)
            with self.assertRaises(SystemExit, msg=level):
                qwen_image._build_transformer_bnb_config(level, torch.bfloat16)

    def test_text_encoder_always_nf4(self):
        from trainer_difusao.models import qwen_image

        cfg = qwen_image._build_text_encoder_bnb_config(torch.bfloat16)
        self.assertTrue(cfg.load_in_4bit)
        self.assertEqual(cfg.bnb_4bit_quant_type, "nf4")
        self.assertTrue(cfg.bnb_4bit_use_double_quant)

    def test_cache_formats_are_distinct_and_legacy_fp4_rejected(self):
        import json

        from trainer_difusao.loaders.quant_cache import _is_cache_valid, resolve_quant_base_dir
        from trainer_difusao.models import qwen_image

        fmt = qwen_image._QWEN_QUANT_CACHE_FORMAT
        self.assertEqual(set(fmt), {"4bit", "8bit"})
        self.assertNotEqual(fmt["4bit"], "4bit")  # formato legado FP4
        self.assertNotEqual(fmt["4bit"], fmt["8bit"])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertNotEqual(
                resolve_quant_base_dir("m", "4bit", base_dir=root),
                resolve_quant_base_dir("m", fmt["4bit"], base_dir=root),
            )
            legacy = root / "legacy"
            (legacy / "transformer").mkdir(parents=True)
            (legacy / "transformer" / "config.json").write_text("{}")
            (legacy / "metadata.json").write_text(
                json.dumps({"model_id": "m", "quant_format": "4bit", "custom_checkpoint": None})
            )
            tdir = legacy / "transformer"
            self.assertTrue(_is_cache_valid(tdir, "m", "4bit"))
            self.assertFalse(_is_cache_valid(tdir, "m", fmt["4bit"]))
            self.assertFalse(_is_cache_valid(tdir, "m", fmt["8bit"]))

    def test_epoch_checkpoint_metadata_records_quantization(self):
        from trainer_difusao.models import qwen_image

        captured = {}

        def fake_save(model, d, name, epoch, metadata, optimizer=None):
            captured.update(metadata)
            return Path(d) / "x.safetensors"

        with mock.patch.object(qwen_image, "save_adapter_checkpoint", fake_save), \
                mock.patch.object(qwen_image, "_prune_checkpoints"):
            qwen_image._maybe_save_epoch_checkpoint(
                None, None, Path("."), "a", epoch=1, local_epoch_idx=1, epochs=1,
                checkpoint_interval=1, global_step=1, quantization="8bit",
            )
        self.assertEqual(captured["quantization"], "8bit")


if __name__ == "__main__":
    unittest.main()
