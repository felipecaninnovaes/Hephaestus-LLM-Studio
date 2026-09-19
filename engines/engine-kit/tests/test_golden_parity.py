"""
Teste de paridade dourada (Golden Parity) — Wave 5 (RD-050).
Garante que o mock_vector em Python produz valores matematicamente idênticos
ao MockEmbedder em Rust (services/api-principal/tests/search_embed.rs).
"""
import struct
import unittest
from engine_kit.mock import mock_vector

class TestGoldenParity(unittest.TestCase):
    def test_mock_vector_golden_values(self):
        # Payload idêntico ao teste do Rust (services/api-principal/tests/search_embed.rs:65)
        payload = b"SPIKE-VECTOR-ALIGNED-PAYLOAD-000" * 64
        vec = mock_vector(payload, dim=512)

        self.assertEqual(len(vec), 512)

        # Valores esperados definidos no teste do Rust
        expected = [
            (0, 0.035050030797719955),
            (1, 0.023601748049259186),
            (2, -0.012896332889795303),
            (3, 0.014493524096906185),
            (510, 0.06539401412010193),
            (511, -0.04202665761113167),
        ]

        for idx, exp_val in expected:
            py_f32 = struct.unpack("f", struct.pack("f", vec[idx]))[0]
            exp_f32 = struct.unpack("f", struct.pack("f", exp_val))[0]
            # Assert exato em bits f32
            py_bits = struct.unpack("I", struct.pack("f", py_f32))[0]
            exp_bits = struct.unpack("I", struct.pack("f", exp_f32))[0]
            self.assertEqual(
                py_bits,
                exp_bits,
                f"Paridade falhou no índice {idx}: Python={py_f32} (bits {py_bits:#x}), Esperado={exp_f32} (bits {exp_bits:#x})"
            )

    def test_known_classes_deterministic_parity(self):
        """Valida que classes canônicas do estúdio produzem normas válidas e determinismo."""
        classes = [b"solda_fria", b"placa_circuito", b"componente_danificado", b"fissura"]
        for cls_name in classes:
            v1 = mock_vector(cls_name)
            v2 = mock_vector(cls_name)
            self.assertEqual(v1, v2)
            self.assertEqual(len(v1), 512)
            # Norma próxima de 1.0 (float precision)
            norm_sq = sum(x * x for x in v1)
            self.assertAlmostEqual(norm_sq, 1.0, places=5)
