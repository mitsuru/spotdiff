#!/usr/bin/env python3
"""PTY lifecycle tests. Terminal responses are simulated; no pixel rendering claim."""
import fcntl
import base64
import os
from pathlib import Path
import pty
import re
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
    def run_pty(self, args, respond=True, keys=None, steps=None, compression=None):
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
                    response = b'\x1b_Gi=31;OK\x1b\\'
                    if b'\x1b_Gi=32' in output and compression is not None:
                        response += (b'\x1b_Gi=32;OK\x1b\\' if compression
                                     else b'\x1b_Gi=32;EINVAL: unsupported compression\x1b\\')
                    os.write(master, response + b'\x1b[6;20;10t\x1b[0n')
                    replied = True
                    image_offset = len(output)
                if keys is not None and not sent and b'Tab:' in output:
                    os.write(master, keys)
                    sent = True
                completed = output[image_offset:].count(b'm=0;') + len(re.findall(
                    rb'\x1b_G[^;]*a=p[^;]*;\x1b\\', output[image_offset:]))
                if steps and replied and completed >= expected_images:
                    action, images = steps[step_index]
                    if isinstance(action, bytes):
                        os.write(master, action)
                    elif isinstance(action, float):
                        time.sleep(action)
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

    def test_images_are_directly_placed_inside_borders(self):
        code, output, _ = self.with_png(steps=[(b'q', 0)], compression=True)
        self.assertEqual(code, 0)
        self.assertFalse(b'U=1' in output, 'virtual placements force Ghostty to rescan cells')
        self.assertFalse('\U0010eeee'.encode() in output, 'image placeholders were emitted')
        placements = re.findall(rb'\x1b\[(\d+);(\d+)H\x1b_G([^;]+);', output)
        images = [(int(row), int(col), dict(field.split(b'=', 1) for field in header.split(b',')))
                  for row, col, header in placements if b'a=T' in header]
        self.assertEqual([(row, col) for row, col, _ in images], [(4, 2), (4, 26)])
        for _, _, fields in images:
            self.assertEqual((fields[b's'], fields[b'v'], fields[b'C']), (b'1', b'1', b'1'))
            self.assertNotIn(b'c', fields, 'cropped image was stretched to fill the pane')
            self.assertNotIn(b'r', fields, 'cropped image was stretched to fill the pane')

    def test_image_updates_are_synchronized_and_idle_does_not_redraw(self):
        code, output, _ = self.with_png(steps=[(.2, 0), (b'q', 0)], compression=True)
        self.assertEqual(code, 0)
        active, images, frames = False, 0, 0
        for match in re.finditer(rb'\x1b\[\?2026([hl])|\x1b_G[^;]*a=T[^;]*;', output):
            if match.group(1) == b'h':
                self.assertFalse(active, 'nested synchronized update')
                active, frames = True, frames + 1
            elif match.group(1) == b'l':
                active = False
            else:
                self.assertTrue(active, 'image sent outside synchronized update')
                images += 1
        self.assertEqual(images, 2)
        self.assertFalse(active, 'terminal left in synchronized update mode')
        self.assertLessEqual(frames, 2, 'unchanged screen was redrawn while idle')

    def test_negotiated_compression_reduces_bytes_without_changing_pixels(self):
        def images(output):
            result, payload, metadata = [], bytearray(), None
            for header, data in re.findall(rb'\x1b_G([^;\x1b]+);([^\x1b]*)\x1b\\', output):
                fields = dict(item.split(b'=', 1) for item in header.split(b','))
                if fields.get(b'a') == b'T':
                    metadata, payload = fields, bytearray()
                if metadata is None or not data:
                    continue
                payload.extend(base64.b64decode(data))
                if fields.get(b'm', b'0') == b'0':
                    raw = zlib.decompress(payload) if metadata.get(b'o') == b'z' else bytes(payload)
                    result.append((int(metadata[b's']), int(metadata[b'v']), raw))
                    metadata = None
            return result

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'compression.png'
            path.write_bytes(png(500, 300))
            compressed_run = self.run_pty([BINARY, str(path), str(path)], steps=[(b'q', 0)], compression=True)
            raw_runs = [(capability, self.run_pty([BINARY, str(path), str(path)], steps=[(b'q', 0)], compression=capability))
                        for capability in (False, None)]
        code, compressed, _ = compressed_run
        self.assertEqual(code, 0)
        self.assertTrue(b'i=32,s=1,v=1,a=q,t=d,f=24,o=z;' in compressed,
                        'compression support was not queried')
        self.assertTrue(re.search(rb'a=T,f=32,o=z,t=d', compressed),
                        'supported compression was not used')
        compressed_images = images(compressed)
        self.assertEqual(len(compressed_images), 2)
        for capability, (code, raw, _) in raw_runs:
            with self.subTest(compression=capability):
                self.assertEqual(code, 0)
                self.assertFalse(re.search(rb'a=T,f=32,o=z,t=d', raw),
                                 'compression used without a positive response')
                self.assertEqual(images(raw), compressed_images)
                self.assertLess(len(compressed), len(raw) / 8)

    def test_zoom_mode_and_resize_then_quit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'a.png'
            path.write_bytes(png(500, 300))
            for compression in (False, True):
                with self.subTest(compression=compression):
                    code, output, _ = self.run_pty([BINARY, str(path), str(path)],
                        compression=compression, steps=[
                        (b'+', 2), (b'l', 2), (b'j', 2), (b'\t', 1),
                        (b'+', 1), (b'f', 1), (b'1', 1),
                        ((10, 40, 400, 200), 1), (b'+', 1),
                        ((16, 60, 600, 320), 1), (b'q', 0),
                    ])
                    self.assertEqual(code, 0)
                    self.assertEqual(output.count(b'a=T,f=32') + output.count(b'a=p,'), 15)

    def test_pan_reuses_uploaded_images(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'pan.png'
            path.write_bytes(png(500, 300))
            code, output, _ = self.run_pty([BINARY, str(path), str(path)],
                compression=True, steps=[
                    (b'+', 2), (b'l', 2), (b'j', 2),
                    (b'h', 2), (b'k', 2), (b'q', 0),
                ])
        self.assertEqual(code, 0)
        self.assertEqual(output.count(b'a=T,f=32'), 4,
                         'pan retransmitted image pixels')
        placements = [dict(field.split(b'=', 1) for field in header.split(b','))
                      for header in re.findall(rb'\x1b_G([^;]*a=p[^;]*);', output)]
        self.assertEqual(len(placements), 8)
        self.assertEqual([(int(p[b'x']), int(p[b'y'])) for p in placements],
                         [(10, 0)] * 2 + [(10, 20)] * 2 + [(0, 20)] * 2 + [(0, 0)] * 2)
        self.assertEqual(len({p[b'i'] for p in placements}), 2)

    def test_resize_reuploads_both_images_when_viewport_pixels_are_unchanged(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'resize.png'
            path.write_bytes(png(500, 300))
            code, output, _ = self.run_pty([BINARY, str(path), str(path)],
                compression=True, steps=[
                    ((12, 49, 490, 240), 1), (b'q', 0),
                ])
        self.assertEqual(code, 0)
        uploads = re.findall(rb'\x1b_G([^;]*a=T[^;]*);', output)
        self.assertEqual(len(uploads), 4,
                         'resize clears terminal images, so both panes must upload again')
        self.assertNotIn(b'a=p,', output,
                         'placement reuse refers to images deleted by the resize clear')
        dimensions = [re.findall(rb'(?:^|,)([sv]=\d+)', header) for header in uploads]
        self.assertEqual(dimensions[:2], dimensions[2:])

    def test_pan_beyond_buffer_refreshes_and_cleans_up_owned_images(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'pan.png'
            path.write_bytes(png(500, 300))
            for compression in (False, True):
                with self.subTest(compression=compression):
                    code, output, _ = self.run_pty([BINARY, str(path), str(path)],
                        compression=compression, steps=[(b'+', 2)] +
                        [(b'l', 2)] * 12 + [(b'h', 2)] * 12 + [(b'q', 0)])
                    self.assertEqual(code, 0)
                    self.assertEqual(output.count(b'a=T,f=32'), 8,
                                     'buffer boundary did not trigger a fresh upload')
                    uploaded, live = set(), set()
                    for header in re.findall(rb'\x1b_G([^;]+);', output):
                        fields = dict(field.split(b'=', 1) for field in header.split(b','))
                        if fields.get(b'a') == b'T':
                            live.add(fields[b'i'])
                            uploaded.add(fields[b'i'])
                        elif fields.get(b'a') == b'p':
                            self.assertIn(fields[b'i'], live, 'placement reused a deleted image')
                            self.assertEqual((fields[b'w'], fields[b'h']), (b'220', b'120'))
                        elif fields.get(b'd') == b'I':
                            live.discard(fields[b'i'])
                    self.assertEqual(len(uploaded), 8)
                    self.assertFalse(live, 'owned image data survived terminal cleanup')

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
                    self.assertIn(b'regular files', result.stderr)
                    self.assertNotIn(b'\x1b', result.stdout)

    @unittest.skipUnless(HELPER, 'pass the test helper binary to cover error and panic')
    def test_error_and_panic_restore(self):
        for mode in ('error', 'panic'):
            code, output, _ = self.run_pty([HELPER, mode])
            self.assertNotEqual(code, 0)
            self.assertIn(('test ' + mode).encode(), output)
            self.assertTrue(b'\x1b[?2026h' in output)
            self.assertTrue(b'\x1b[?2026l' in output)
            self.assertLess(output.index(b'\x1b[?2026l'), output.index(b'\x1b[?1049l'),
                            'synchronized update was not ended before leaving the screen')

if __name__ == '__main__':
    unittest.main()
