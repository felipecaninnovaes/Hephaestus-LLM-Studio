import math
import unittest

from engine_kit.mock import is_mock, mock_vector, seed_bytes, synthetic_loss, synthetic_yolo_metrics


class TestMockPrimitives(unittest.TestCase):
    def test_is_mock(self):
        self.assertTrue(is_mock("1"))
        self.assertTrue(is_mock("true"))
        self.assertTrue(is_mock("True"))
        self.assertTrue(is_mock("yes"))
        self.assertTrue(is_mock("YES"))
        self.assertFalse(is_mock("0"))
        self.assertFalse(is_mock("false"))
        self.assertFalse(is_mock(""))
        self.assertFalse(is_mock("no"))

    def test_seed_bytes_deterministic(self):
        b1 = seed_bytes(42, 32)
        b2 = seed_bytes(42, 32)
        b3 = seed_bytes(43, 32)
        self.assertEqual(len(b1), 32)
        self.assertEqual(b1, b2)
        self.assertNotEqual(b1, b3)

    def test_mock_vector_normalization(self):
        v = mock_vector(b"hello world", dim=512)
        self.assertEqual(len(v), 512)
        norm = math.sqrt(sum(x * x for x in v))
        self.assertAlmostEqual(norm, 1.0, places=4)

        # Determinismo
        v2 = mock_vector(b"hello world", dim=512)
        self.assertEqual(v, v2)

    def test_synthetic_loss(self):
        loss_ep1 = synthetic_loss(1234, 1, 10)
        loss_ep10 = synthetic_loss(1234, 10, 10)
        self.assertGreater(loss_ep1, loss_ep10)
        self.assertGreater(loss_ep10, 0.0)

    def test_synthetic_yolo_metrics(self):
        m1 = synthetic_yolo_metrics(1234, 1, 10)
        m10 = synthetic_yolo_metrics(1234, 10, 10)
        self.assertIn("box_loss", m1)
        self.assertIn("mAP50", m1)
        self.assertGreater(m1["box_loss"], m10["box_loss"])
        self.assertLess(m1["mAP50"], m10["mAP50"])
