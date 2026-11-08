#!/usr/bin/env python3
"""Actual text/byte cleanup fallback controls; no provider qualification."""
import json
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import unittest

import processes


class CleanupControls(unittest.TestCase):
    def check_cleanup(self, text):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            out = b'partial UTF8 \xc3\xa9 and undecodable \xff\n'
            err = b'partial stderr \xff\n'
            # stdout readiness proves both writes occurred; never consume bytes
            # with an independent reader before the cleanup owner receives them.
            program = 'import os,time;os.write(2,' + repr(err) + ');os.write(1,' + repr(out) + ');time.sleep(30)'
            options = {'text': True, 'encoding': 'utf-8', 'errors': 'surrogateescape'} if text else {}
            process = processes.start([sys.executable, '-c', program], stdout=subprocess.PIPE, stderr=subprocess.PIPE, **options)
            process._owned_stdout_prefix = 'observed-prefix\n' if text else b'observed-prefix\n'
            try:
                self.assertTrue(select.select([process.stdout], [], [], 5)[0], 'producer did not become ready')
                processes.cleanup_all(folder)
                prefix = folder / ('owned-cleanup-' + str(process.pid))
                self.assertEqual(prefix.with_suffix('.stdout').read_bytes(), b'observed-prefix\n' + out)
                self.assertEqual(prefix.with_suffix('.stderr').read_bytes(), err)
                row = json.loads(prefix.with_suffix('.json').read_text())
                self.assertIsNone(row['error'])
                self.assertIsNotNone(row['returncode'])
                self.assertNotIn(process.pid, processes.ACTIVE)
            finally:
                if process.pid in processes.ACTIVE:
                    processes.stop(process)
                    processes.forget(process)

    def test_text_actor_fallback_preserves_codec_bytes_and_releases_owner(self):
        self.check_cleanup(True)

    def test_byte_command_fallback_keeps_observed_prefix_and_releases_owner(self):
        self.check_cleanup(False)


if __name__ == '__main__':
    unittest.main()
