import io
import sys
import unittest
from unittest.mock import MagicMock, patch

from engine_kit import (
    cleanup_cuda,
    get_vram_usage,
    log_vram,
    release_memory,
    require_cuda,
    vram_allocated_gb,
    vram_guard,
    vram_reserved_gb,
)


class TestVRAM(unittest.TestCase):
    def test_vram_guard_basic(self):
        """vram_guard não deve levantar exceções no fluxo normal."""
        executed = False
        with vram_guard(tag="test"):
            executed = True
        self.assertTrue(executed)

    def test_vram_guard_propagates_exception(self):
        """vram_guard deve propagar exceções internas e ainda executar cleanup."""
        with self.assertRaises(RuntimeError):
            with vram_guard(tag="test_err"):
                raise RuntimeError("Erro dentro do bloco protegido")

    def test_release_memory_dummy_objects(self):
        """release_memory deve aceitar zero ou múltiplos objetos sem falhar."""
        dummy1 = [1, 2, 3]
        dummy2 = {"key": "value"}
        dummy3 = object()
        # Sem argumentos
        release_memory()
        # Com múltiplos objetos dummy
        release_memory(dummy1, dummy2, dummy3)

    def test_get_vram_usage_structure(self):
        """get_vram_usage retorna chaves padronizadas."""
        usage = get_vram_usage()
        self.assertIn("allocated_gb", usage)
        self.assertIn("reserved_gb", usage)

    def test_log_vram_execution(self):
        """log_vram roda sem erro com ou sem GPU."""
        log_vram(tag="test_log")

    def test_vram_guard_with_mocked_cuda(self):
        """Verifica se empty_cache e ipc_collect são invocados quando CUDA está disponível."""
        mock_torch = MagicMock()
        mock_torch.cuda.is_available.return_value = True

        with patch.dict(sys.modules, {"torch": mock_torch}):
            with vram_guard(tag="cuda_mock"):
                pass

            mock_torch.cuda.empty_cache.assert_called_once()
            mock_torch.cuda.ipc_collect.assert_called_once()

    def test_release_memory_with_mocked_cuda(self):
        """Verifica se cleanup de CUDA ocorre no release_memory com CUDA ativo."""
        mock_torch = MagicMock()
        mock_torch.cuda.is_available.return_value = True

        with patch.dict(sys.modules, {"torch": mock_torch}):
            obj = [42]
            release_memory(obj)

            mock_torch.cuda.empty_cache.assert_called_once()
            mock_torch.cuda.ipc_collect.assert_called_once()

    def test_vram_allocated_and_reserved_with_mock_cuda(self):
        """Verifica cálculo correto de GB com divisor 1024³."""
        mock_torch = MagicMock()
        mock_torch.cuda.is_available.return_value = True
        mock_torch.cuda.memory_allocated.return_value = 2 * (1024 ** 3)
        mock_torch.cuda.memory_reserved.return_value = 4 * (1024 ** 3)

        with patch.dict(sys.modules, {"torch": mock_torch}):
            alloc = vram_allocated_gb()
            res = vram_reserved_gb()
            self.assertEqual(alloc, 2.0)
            self.assertEqual(res, 4.0)

            usage = get_vram_usage()
            self.assertEqual(usage["allocated_gb"], 2.0)
            self.assertEqual(usage["reserved_gb"], 4.0)

            # Verifica log_vram com saída formatada
            stdout_capture = io.StringIO()
            with patch("sys.stdout", stdout_capture):
                log_vram(tag="gpu_test")
            output = stdout_capture.getvalue()
            self.assertIn("GPU_TEST", output)
            self.assertIn("2.00GB", output)
            self.assertIn("4.00GB", output)

    def test_require_cuda_fails_when_cuda_unavailable(self):
        """require_cuda deve dar sys.exit(1) se CUDA indisponível."""
        mock_torch = MagicMock()
        mock_torch.cuda.is_available.return_value = False

        with patch.dict(sys.modules, {"torch": mock_torch}):
            with self.assertRaises(SystemExit) as ctx:
                require_cuda("Teste Operação")
            self.assertEqual(ctx.exception.code, 1)
