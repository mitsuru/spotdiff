#!/usr/bin/env python3
"""PTY lifecycle tests. Terminal responses are simulated; no pixel rendering claim."""
import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest
import zlib

BINARY = str(Path(sys.argv[1]).resolve())
HELPER = str(Path(sys.argv[2]).resolve()) if len(sys.argv) > 2 else None
sys.argv[1:] = []

def png(width=1, height=1):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress((b'\x00' + b'\xff\x00\x00\xff' * width) * height)) + chunk(b'IEND', b'')

class TerminalTests(unittest.TestCase):
    def run_pty(self, args, respond=True, keys=None, steps=None):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 12, 48, 480, 240))
        initial = termios.tcgetattr(slave)
        env = dict(os.environ)
        for name in ('TERM_PROGRAM', 'TMUX', 'WEZTERM_EXECUTABLE', 'ITERM_SESSION_ID', 'LC_TERMINAL', 'KONSOLE_VERSION'):
            env.pop(name, None)
        env['TERM'] = 'xterm-kitty'
        process = subprocess.Popen(args, stdin=slave, stdout=slave, stderr=slave, env=env)
        output = bytearray()
        replied = sent = False
        step_index, expected_images, image_offset = 0, 2, 0
        start = time.monotonic()
        try:
            while process.poll() is None:
                self.assertLess(time.monotonic() - start, 5, 'terminal process did not exit')
                if select.select([master], [], [], .02)[0]:
                    output.extend(os.read(master, 65536))
                if respond and not replied and b'\x1b[5n' in output:
                    os.write(master, b'\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n')
                    replied = True
                    image_offset = len(output)
                if keys is not None and not sent and b'Tab:' in output:
                    os.write(master, keys)
                    sent = True
                if steps and replied and output[image_offset:].count(b'm=0;') >= expected_images:
                    action, images = steps[step_index]
                    if isinstance(action, bytes):
                        os.write(master, action)
                    else:
                        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', *action))
                    expected_images += images
                    step_index += 1
                    if step_index == len(steps):
                        steps = None
            while select.select([master], [], [], .02)[0]:
                output.extend(os.read(master, 65536))
            self.assertEqual(termios.tcgetattr(slave), initial, 'termios was not restored')
            self.assertIn(b'\x1b[?1049l', output, 'alternate screen was not restored')
            self.assertIn(b'\x1b[?25h', output, 'cursor was not restored')
            return process.returncode, bytes(output), time.monotonic() - start
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            os.close(master)
            os.close(slave)

    def with_png(self, **kwargs):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'a.png'
            path.write_bytes(png())
            return self.run_pty([BINARY, str(path), str(path)], **kwargs)

    def test_no_response_restores_within_deadline(self):
        code, output, elapsed = self.with_png(respond=False)
        self.assertNotEqual(code, 0)
        self.assertLess(elapsed, 5)
        self.assertIn(b'Kitty', output)

    def test_quit_and_ctrl_c_restore(self):
        for key in (b'q', b'\x03', b'\x1b'):
            with self.subTest(key=key):
                code, _, _ = self.with_png(keys=key)
                self.assertEqual(code, 0)

    def test_zoom_mode_and_resize_then_quit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'a.png'
            path.write_bytes(png(500, 300))
            code, output, _ = self.run_pty([BINARY, str(path), str(path)], steps=[
                (b'+', 2), (b'l', 2), (b'j', 2), (b'\t', 1),
                (b'+', 1), (b'f', 1), (b'1', 1),
                ((10, 40, 400, 200), 1), (b'+', 1),
                ((16, 60, 600, 320), 1), (b'q', 0),
            ])
            self.assertEqual(code, 0)
            self.assertEqual(output.count(b'a=T,U=1'), 15)

    def test_non_regular_inputs_fail_before_terminal_setup(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'pipe.png'
            os.mkfifo(path)
            subprocess.run(['git', 'init', '-q', directory], check=True)
            for args in ([BINARY, str(path), str(path)],
                         [BINARY, 'git', '--', str(path)]):
                with self.subTest(args=args):
                    result = subprocess.run(args, cwd=directory, stdin=subprocess.DEVNULL,
                                            capture_output=True, timeout=2)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('通常ファイル'.encode(), result.stderr)
                    self.assertNotIn(b'\x1b', result.stdout)

    @unittest.skipUnless(HELPER, 'pass the test helper binary to cover error and panic')
    def test_error_and_panic_restore(self):
        for mode in ('error', 'panic'):
            code, output, _ = self.run_pty([HELPER, mode])
            self.assertNotEqual(code, 0)
            self.assertIn(('test ' + mode).encode(), output)

if __name__ == '__main__':
    unittest.main()
