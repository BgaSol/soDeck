"""Exercise the shipped startup scripts in real Linux Bash/Zsh PTYs."""
import errno
import os
from pathlib import Path
import pty
import re
import select
import shlex
import signal
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
TOKEN = "a1b2c3d4"
REPORT = re.compile(rb"\x1b\]633;SODECK;" + TOKEN.encode() + rb";([^\x07]*)\x07")


def launch(shell):
    folder = ROOT / "plugins/terminal/shell"
    text = (folder / "launch.sh").read_text()
    for key, name in [("INTEGRATION", "integration"), ("BASH", "bash"), ("ZSH", "zsh")]:
        text = text.replace("@" + key + "@", (folder / (name + ".sh")).read_text())
    return text.replace("@SHELL@", shell).replace("@TOKEN@", TOKEN)


class Shell:
    def __init__(self, shell, home):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(home)
            os.environ.update(HOME=str(home), SHELL=shell, TERM="xterm-256color", TRACE_FILE=str(home / "trace"))
            os.environ.pop("ZDOTDIR", None)
            os.execv("/bin/sh", ["sh", "-c", launch(shell)])
        attrs = termios.tcgetattr(self.fd)
        attrs[3] &= ~termios.ECHO
        termios.tcsetattr(self.fd, termios.TCSANOW, attrs)
        self.buffer = b""

    def read(self, predicate, timeout=5):
        result = b""
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if select.select([self.fd], [], [], 0.05)[0]:
                try:
                    result += os.read(self.fd, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
            if predicate(result):
                self.buffer += result
                return result
        self.buffer += result
        raise AssertionError(result.decode(errors="replace"))

    def ready(self):
        return self.read(lambda b: any(r.startswith(b"ready;") for r in REPORT.findall(b)))

    def command(self, text):
        os.write(self.fd, text.encode() + b"\n")
        return self.ready()

    def close(self):
        os.write(self.fd, b"exit\n")
        until = time.monotonic() + 3
        while time.monotonic() < until:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                os.close(self.fd)
                assert os.waitstatus_to_exitcode(status) == 0, status
                return
            time.sleep(0.03)
        os.kill(self.pid, signal.SIGKILL)
        os.waitpid(self.pid, 0)
        os.close(self.fd)
        raise AssertionError("shell did not exit")


def path_of(output):
    report = [v for v in REPORT.findall(output) if v.startswith(b"ready;")][-1].split(b";")
    return bytes.fromhex(report[1].decode()).decode(), Path(bytes.fromhex(report[2].decode()).decode())


for shell, array in [("/bin/bash", False), ("/bin/bash", True), ("/bin/zsh", False)]:
    with tempfile.TemporaryDirectory(prefix="sodeck-shell-test-") as folder:
        home = Path(folder)
        hook = 'user_prompt() { local result=$?; printf "USER:%s\\n" "$result"; return "$result"; }\n'
        (home / ".bash_profile").write_text('echo profile >> "$TRACE_FILE"\n. "$HOME/.bashrc"\n')
        (home / ".bashrc").write_text('echo rc >> "$TRACE_FILE"\n' + hook + ('PROMPT_COMMAND=(user_prompt)\n' if array else 'PROMPT_COMMAND=user_prompt\n') + "PS1='custom> '\n")
        for stage in [".zshenv", ".zprofile", ".zshrc", ".zlogin"]:
            # Ubuntu runners have writable global completion paths; completion is not this fixture's subject.
            (home / stage).write_text(f'echo {stage} >> "$TRACE_FILE"\n' + ('skip_global_compinit=1\n' if stage == ".zshenv" else '') + (hook + 'precmd_functions=(user_prompt)\nPS1="custom> "\n' if stage == ".zshrc" else ""))
        session = Shell(shell, home)
        try:
            first = session.ready()
            actual, witness = path_of(first)
            assert actual == str(home), (actual, home)
            assert witness.read_text() == TOKEN
            trace = (home / "trace").read_text().splitlines()
            assert trace == (["profile", "rc"] if shell.endswith("bash") else [".zshenv", ".zprofile", ".zshrc", ".zlogin"]), trace
            assert b"USER:1" in session.command("false"), "prompt hook lost exit status"
            target = home / "目录 with % # ' quotes"
            target.mkdir()
            assert path_of(session.command("cd " + shlex.quote(str(target))))[0] == str(target)
            assert path_of(session.command("cd /does-not-exist"))[0] == str(target)
            (home / "link").symlink_to(target)
            assert path_of(session.command("cd " + shlex.quote(str(home / "link"))))[0] == str(target)
            assert path_of(session.command("function move_here() { cd " + shlex.quote(str(home)) + "; }; move_here"))[0] == str(home)
            os.write(session.fd, b"bash --noprofile --norc\n")
            session.read(lambda b: b"busy;" in b)
            os.write(session.fd, b"printf 'NESTED-DONE\\n'\n")
            nested = session.read(lambda b: b"NESTED-DONE" in b)
            assert not any(r.startswith(b"ready;") for r in REPORT.findall(nested)), nested
            session.command("exit")
            os.write(session.fd, b"sleep 30\n")
            session.read(lambda b: b"busy;" in b)
            time.sleep(0.15)  # Wait for sleep to own the foreground process group.
            os.write(session.fd, b"\x03")
            session.ready()
            assert b"LOGIN-OK" in session.command("if " + ("shopt -q login_shell" if shell.endswith("bash") else "[[ -o login ]]" ) + "; then echo LOGIN-OK; fi")
            session.command("true")
        finally:
            session.close()
        assert not witness.exists(), "startup files were not cleaned up"
        print(f"Passed {shell} ({'array' if array else 'scalar'} hook): default startup, user profiles, status, paths, nested shell, Ctrl-C, cleanup", flush=True)
