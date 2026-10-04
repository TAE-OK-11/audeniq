import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('kms_parallel', Path(__file__).resolve().parents[1] / 'kms-keyring.py')
kms = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kms)
ARN = 'arn:aws:kms:ap-northeast-1:123456789012:key/00000000-0000-0000-0000-000000000001'


def bundle(n):
    return {'version': 2, 'active_version': n, 'keys': {
        str(i): {'provider': 'aws', 'key_id': ARN, 'ciphertext': base64.b64encode(bytes([i])).decode()}
        for i in range(1, n + 1)
    }}


class ParallelKeyTests(unittest.TestCase):
    def test_materialization_is_parallel_bounded_and_preserves_versions(self):
        barrier = threading.Barrier(2, timeout=5)
        lock = threading.Lock()
        state = {'active': 0, 'peak': 0}
        def unwrap(key, version, scratch):
            with lock:
                state['active'] += 1
                state['peak'] = max(state['peak'], state['active'])
            try:
                if int(version) <= 2:
                    barrier.wait()
                return bytes([int(version)]) * 32
            finally:
                with lock: state['active'] -= 1
        with patch.object(kms, 'unwrap_key', side_effect=unwrap), patch.object(kms, 'atomic_write') as write:
            kms.materialize(bundle(5), '/dev/shm', 'destination', 10001, concurrency=2)
        self.assertEqual(state['peak'], 2)
        self.assertEqual(state['active'], 0)
        self.assertEqual(write.call_args.args, ('destination', {
            'version': 1, 'active_version': 5,
            'keys': {str(i): (bytes([i]) * 32).hex() for i in range(1, 6)}
        }, 10001))

    def test_failure_preserves_existing_keyring_and_waits_for_other_work(self):
        barrier = threading.Barrier(2, timeout=5)
        completed = threading.Event()
        def unwrap(key, version, scratch):
            barrier.wait()
            if version == '1': raise ValueError('KMS unavailable')
            completed.set()
            return b'X' * 32
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'keyring.json'
            destination.write_bytes(b'existing-private-keyring')
            with patch.object(kms, 'unwrap_key', side_effect=unwrap), self.assertRaises(ValueError):
                kms.materialize(bundle(2), directory, destination, 0, concurrency=2)
            self.assertTrue(completed.is_set())
            self.assertEqual(destination.read_bytes(), b'existing-private-keyring')

    def test_invalid_concurrency_fails_before_any_key_operation(self):
        for concurrency in [0, 5, True, 1.5, '2']:
            with patch.object(kms, 'unwrap_key') as unwrap, self.assertRaises(ValueError):
                kms.materialize(bundle(1), '/dev/shm', 'destination', 0, concurrency)
            unwrap.assert_not_called()

    def test_serial_mode_uses_no_executor_and_keeps_output_order(self):
        with patch.object(kms, 'ThreadPoolExecutor') as executor:
            keys = kms.map_keys(bundle(4)['keys'], lambda version, key: version, 1)
        executor.assert_not_called()
        self.assertEqual(list(keys), ['1', '2', '3', '4'])

    def test_rewrap_preserves_material_and_active_version(self):
        original = bundle(4)
        snapshot = json.dumps(original, sort_keys=True)
        def wrap(provider, key_id, version, plaintext, scratch):
            self.assertEqual(plaintext, bytes([int(version)]) * 32)
            return {'provider': provider, 'key_id': key_id, 'ciphertext': base64.b64encode(plaintext).decode()}
        with patch.object(kms, 'unwrap_key', side_effect=lambda key, version, scratch: bytes([int(version)]) * 32), patch.object(kms, 'wrap_key', side_effect=wrap):
            result = kms.rewrap_bundle(original, 'aws', ARN, '/dev/shm', concurrency=2)
        self.assertEqual(result['active_version'], 4)
        self.assertEqual(json.dumps(original, sort_keys=True), snapshot)
        self.assertEqual(set(result['keys']), set(original['keys']))


if __name__ == '__main__':
    unittest.main()
