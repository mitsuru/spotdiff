#!/usr/bin/env python3
"""Run the real lazygit with an isolated config; Kitty replies are simulated."""
import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import time
import unittest

from terminal_pty import BINARY, png

ROOT = Path(__file__).resolve().parents[1]
LAZYGIT = shutil.which('lazygit')

@unittest.skipUnless(LAZYGIT, 'lazygit is not installed')
class LazygitTests(unittest.TestCase):
    def test_config_launch_and_return(self):
        scenarios = [('unstaged', b'I', b'index:', b'worktree:'),
                     ('staged', b'I', b'HEAD:', b'index:'),
                     ('both', b'I', b'index:', b'worktree:'),
                     ('both', b'\x13', b'HEAD:', b'index:')]
        for changes, key, before, after in scenarios:
            with self.subTest(changes=changes, key=key):
                self.launch_and_return(changes, key, before, after)

    def launch_and_return(self, changes, key, before, after):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            repo, config_dir = directory / 'repo', directory / 'config'
            repo.mkdir()
            config_dir.mkdir()
            env = dict(os.environ)
            env.update(TERM='xterm-kitty', PATH=str(Path(BINARY).parent) + os.pathsep + env['PATH'],
                       XDG_CONFIG_HOME=str(config_dir), XDG_STATE_HOME=str(directory / 'state'),
                       GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
                       GIT_AUTHOR_NAME='Test', GIT_AUTHOR_EMAIL='test@example.invalid',
                       GIT_COMMITTER_NAME='Test', GIT_COMMITTER_EMAIL='test@example.invalid')
            for name in ('TMUX', 'TERM_PROGRAM', 'WEZTERM_EXECUTABLE', 'ITERM_SESSION_ID',
                         'KONSOLE_VERSION', 'LC_TERMINAL'):
                env.pop(name, None)

            def git(*args):
                subprocess.run(['git', '-C', str(repo), *args], env=env, check=True,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

            name = "a '日本語.png"
            git('init', '-q')
            path = repo / name
            path.write_bytes(png())
            git('add', '--', name)
            git('commit', '-qm', 'initial')
            path.write_bytes(png() + b'changed')
            if changes in ('staged', 'both'):
                git('add', '--', name)
            if changes == 'both':
                path.write_bytes(png() + b'worktree')
            config = config_dir / 'config.yml'
            config.write_text('disableStartupPopups: true\ngui:\n  showRandomTip: false\n'
                              'git:\n  autoFetch: false\n' + (ROOT / 'examples/lazygit.yml').read_text())
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 30, 100, 1000, 600))
            initial = termios.tcgetattr(slave)

            def controlling_tty():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)

            process = subprocess.Popen([LAZYGIT, '--use-config-dir', str(config_dir),
                                        '--use-config-file', str(config)], cwd=repo, env=env,
                                       stdin=slave, stdout=slave, stderr=slave,
                                       preexec_fn=controlling_tty)
            output, phase = bytearray(), 'lazygit'
            replied, offset, image_offset, returned = False, 0, 0, False
            deadline = time.monotonic() + 12
            try:
                while process.poll() is None:
                    self.assertLess(time.monotonic(), deadline, 'timeout: ' + phase + repr(output[-1500:]))
                    if select.select([master], [], [], .02)[0]:
                        output.extend(os.read(master, 65536))
                    # Wait for Git refresh, not the empty initial panel headers.
                    if phase == 'lazygit' and b'Binary' in output:
                        os.write(master, b'2' + key)
                        phase, offset = 'starting spotdiff', len(output)
                    if not replied and b'\x1b_Gi=31' in output and b'\x1b[5n' in output:
                        os.write(master, b'\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n')
                        replied = True
                        image_offset = len(output)
                    if phase == 'starting spotdiff' and replied and output[image_offset:].count(b'm=0;') >= 2:
                        self.assertIn(before, output[offset:])
                        self.assertIn(after, output[offset:])
                        self.assertIn(b'Tab:', output[offset:])
                        os.write(master, b'q')
                        phase, offset = 'return prompt', len(output)
                    if phase == 'return prompt' and b'Press enter to return to lazygit' in output[offset:]:
                        self.assertIn(b'\x1b[?1049l', output[offset:])
                        self.assertIn(b'\x1b[?25h', output[offset:])
                        os.write(master, b'\r')
                        phase, offset = 'returning', len(output)
                    if phase == 'returning' and b'Files' in output[offset:]:
                        returned, phase = True, 'returned to lazygit'
                        os.write(master, b'q')
                self.assertTrue(returned, 'did not return: ' + phase)
                self.assertEqual(process.returncode, 0)
                self.assertEqual(termios.tcgetattr(slave), initial)
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait()
                os.close(master)
                os.close(slave)

if __name__ == '__main__':
    unittest.main()
