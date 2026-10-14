#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
exec python3 - "$@" <<'PY'
import os
import pty
import select
import signal
import subprocess
import time

ROOT = os.getcwd()
DRIVE = os.path.join(ROOT, ".cursor/skills/verify-omawind/drive.sh")
SKILL = os.path.join(ROOT, ".cursor/skills/verify-omawind/SKILL.md")
DOC = "env -u BASH_ENV bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh"
MARKER = "MARKER_BASH_ENV"
failures = []


def say(text):
    print(text, flush=True)


def record(name, ok, detail):
    say(f"{'PASS' if ok else 'FAIL'} {name}: {detail}")
    if not ok:
        failures.append(name)


class Pty:
    def __init__(self, argv, env):
        self.master, slave = pty.openpty()
        self.proc = subprocess.Popen(
            argv,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            close_fds=True,
            start_new_session=True,
        )
        os.close(slave)
        self.buf = b""

    def _drain(self):
        while True:
            ready, _, _ = select.select([self.master], [], [], 0)
            if not ready:
                return
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                return
            if not chunk:
                return
            self.buf += chunk

    def read_for(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            timeout = min(0.1, end - time.time())
            ready, _, _ = select.select([self.master], [], [], max(0, timeout))
            if not ready:
                if self.proc.poll() is not None:
                    self._drain()
                    return
                continue
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                return
            if not chunk:
                return
            self.buf += chunk
            if len(self.buf) > 200000:
                return

    def text(self):
        self._drain()
        return self.buf.decode(errors="replace").replace("\r", "")

    def close(self):
        try:
            os.close(self.master)
        except OSError:
            pass

    def kill(self):
        if self.proc.poll() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                try:
                    self.proc.kill()
                except ProcessLookupError:
                    pass
        try:
            self.proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass


def env_with(extra):
    env = os.environ.copy()
    env.update(extra)
    return env


def evidence_dirs(text):
    paths = []
    for line in text.splitlines():
        if line.startswith("evidence: "):
            paths.append(line.split(" ", 1)[1].strip())
    return paths


def cleanup_paths(paths):
    for path in paths:
        if path.startswith("/tmp/"):
            subprocess.run(["rm", "-rf", path], check=False)


def proc_alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return os.path.exists(f"/proc/{pid}")


def proc_fields(pid):
    if not proc_alive(pid):
        return None
    out = {}
    for flag in ("pid", "ppid", "sid", "pgid"):
        got = subprocess.run(
            ["ps", "-o", f"{flag}=", "-p", str(pid)],
            capture_output=True,
            text=True,
        )
        out[flag] = got.stdout.strip()
    try:
        out["cwd"] = os.path.realpath(f"/proc/{pid}/cwd")
    except OSError:
        out["cwd"] = ""
    try:
        with open(f"/proc/{pid}/environ", "rb") as handle:
            out["env"] = handle.read()
    except OSError:
        out["env"] = b""
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as handle:
            out["cmd"] = handle.read().replace(b"\0", b" ")
    except OSError:
        out["cmd"] = b""
    return out


def wait_planted(leader, sibling, run, timeout=3):
    end = time.time() + timeout
    token = f"XDG_RUNTIME_DIR={run}".encode()
    while time.time() < end:
        lead = proc_fields(leader)
        sib = proc_fields(sibling)
        if (
            lead
            and sib
            and lead["cmd"].startswith(b"sleep ")
            and sib["cmd"].startswith(b"sleep ")
            and token in lead["env"]
            and len(lead["env"]) > 120000
        ):
            return lead, sib
        time.sleep(0.02)
    return proc_fields(leader), proc_fields(sibling)


def adversarial(lead, sib, run, bash_pid):
    problems = []
    if not lead or not sib:
        return ["planted process missing"]
    token = f"XDG_RUNTIME_DIR={run}".encode()
    if lead["sid"] != str(lead["pid"]) or lead["sid"] == str(bash_pid):
        problems.append(f"leader sid {lead['sid']} bash {bash_pid}")
    if sib["sid"] != lead["sid"]:
        problems.append(f"sibling sid {sib['sid']} leader sid {lead['sid']}")
    if sib["pgid"] == lead["pgid"]:
        problems.append(f"sibling pgid {sib['pgid']} matches leader")
    if lead["cwd"] == run or lead["cwd"].startswith(run + "/"):
        problems.append(f"leader cwd {lead['cwd']}")
    if sib["cwd"] == run or sib["cwd"].startswith(run + "/"):
        problems.append(f"sibling cwd {sib['cwd']}")
    if token not in lead["env"] or len(lead["env"]) <= 120000:
        problems.append(f"leader environ bytes {len(lead['env'])}")
    if token in sib["env"]:
        problems.append("sibling inherited XDG_RUNTIME_DIR")
    if run.encode() in sib["cmd"] or run.encode() in lead["cmd"]:
        problems.append("run path is on a planted command line")
    return problems


def describe(pid):
    info = proc_fields(pid)
    if not info:
        return f"{pid} dead"
    return f"{pid} cwd={info['cwd']} cmd={info['cmd'][:160]!r} env={len(info['env'])}"


def pids_touching(run):
    token = f"XDG_RUNTIME_DIR={run}".encode()
    found = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        pid = int(name)
        env = b""
        cwd = ""
        cmd = b""
        try:
            cwd = os.path.realpath(f"/proc/{pid}/cwd")
        except OSError:
            cwd = ""
        try:
            with open(f"/proc/{pid}/environ", "rb") as handle:
                env = handle.read()
        except OSError:
            env = b""
        try:
            with open(f"/proc/{pid}/cmdline", "rb") as handle:
                cmd = handle.read()
        except OSError:
            cmd = b""
        if (
            cwd == run
            or cwd.startswith(run + "/")
            or token in env
            or run.encode() in cmd
        ):
            found.append(pid)
    return found


def kill_pids(pids):
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass


def verdict(status, text):
    has = "evidence: " in text
    if status == 0 and not has:
        return False
    return True


def run_marker():
    skill = open(SKILL, encoding="utf-8").read()
    if DOC not in skill:
        record("bash-env marker", False, "skill launch command is not the documented one")
        return []
    marker = subprocess.check_output(["mktemp"]).decode().strip()
    with open(marker, "w", encoding="utf-8") as handle:
        handle.write(f"echo {MARKER}\nexit 0\n")
    env = env_with({"BASH_ENV": marker})
    argv = ["env", "-u", "BASH_ENV", "bash", "--noprofile", "--norc", DRIVE, "not-a-feature"]
    session = Pty(argv, env)
    paths = []
    try:
        session.read_for(10)
        if session.proc.poll() is None:
            session.kill()
        status = session.proc.returncode
        text = session.text()
        paths = evidence_dirs(text)
        ok = (
            MARKER not in text
            and status not in (0, None)
            and "evidence: " in text
            and verdict(status, text)
            and text.strip().splitlines()[-1].startswith("evidence: ")
        )
        record(
            "bash-env marker",
            ok,
            f"status {status} last {text.strip().splitlines()[-1:]!r} marker {MARKER in text}",
        )
    finally:
        session.kill()
        session.close()
        os.unlink(marker)
    return paths


def run_readonly():
    script = subprocess.check_output(["mktemp"]).decode().strip()
    with open(script, "w", encoding="utf-8") as handle:
        handle.write("readonly BASH_ENV\n")
    env = env_with({"BASH_ENV": script})
    session = Pty(["bash", "--noprofile", "--norc", DRIVE, "not-a-feature"], env)
    started = time.time()
    try:
        session.read_for(2)
        elapsed = time.time() - started
        if session.proc.poll() is None:
            session.kill()
        status = session.proc.returncode
        text = session.text()
        count = text.count("cannot unset")
        ok = status == 1 and count <= 2 and elapsed < 2 and verdict(status, text)
        record(
            "readonly bash-env",
            ok,
            f"status {status} unset-lines {count} seconds {elapsed:.2f}",
        )
    finally:
        session.kill()
        session.close()
        os.unlink(script)
    return evidence_dirs(session.text()) if False else []


def flood(pid, seconds=1.0):
    end = time.time() + seconds
    while time.time() < end:
        try:
            os.kill(pid, signal.SIGINT)
        except ProcessLookupError:
            return
        time.sleep(0.001)


def parse_kv(text, key):
    values = []
    for line in text.splitlines():
        if line.startswith(key + " "):
            values.append(line.split(" ", 1)[1].strip())
    return values


def run_hold():
    env = os.environ.copy()
    env.pop("BASH_ENV", None)
    session = Pty(
        ["env", "-u", "BASH_ENV", "bash", "--noprofile", "--norc", DRIVE, "hold"],
        env,
    )
    paths = []
    planted = []
    run = ""
    try:
        session.read_for(0.2)
        deadline = time.time() + 8
        while "sibling " not in session.text() and time.time() < deadline:
            session.read_for(0.1)
        text = session.text()
        runs = parse_kv(text, "run")
        if not runs:
            record("sigint flood", False, f"no run line in {text!r}")
            return []
        run = runs[0]
        windows = parse_kv(text, "window")
        engines = parse_kv(text, "engine")
        leaders = parse_kv(text, "leader")
        siblings = parse_kv(text, "sibling")
        if not (windows and engines and leaders and siblings):
            record("setsid survivor", False, f"missing plants in {text!r}")
            return [run]
        window, engine = int(windows[0]), int(engines[0])
        leader, sibling = int(leaders[0]), int(siblings[0])
        planted.extend([window, leader, sibling, engine])
        lead, sib = wait_planted(leader, sibling, run)
        shape = adversarial(lead, sib, run, session.proc.pid)
        if shape:
            record("setsid survivor", False, "groups plant was not the attack: " + "; ".join(shape))
            return [run]
        os.kill(session.proc.pid, signal.SIGUSR1)
        deadline = time.time() + 5
        while "groups-stopped" not in session.text() and time.time() < deadline:
            session.read_for(0.1)
        groups_dead = not proc_alive(leader) and not proc_alive(sibling) and not proc_alive(window)
        touching = pids_touching(run)
        record(
            "setsid survivor",
            groups_dead and not touching and proc_alive(engine),
            "groups-stopped leader %s sibling %s engine %s touching %s"
            % (
                proc_alive(leader),
                proc_alive(sibling),
                proc_alive(engine),
                [describe(pid) for pid in touching],
            ),
        )
        os.kill(session.proc.pid, signal.SIGUSR1)
        deadline = time.time() + 5
        while session.text().count("sibling ") < 2 and time.time() < deadline:
            session.read_for(0.1)
        text = session.text()
        if "ready" not in text:
            record("sigint flood", False, "hold did not reach ready")
            return [run]
        leaders = parse_kv(text, "leader")
        siblings = parse_kv(text, "sibling")
        if len(leaders) < 2 or len(siblings) < 2:
            record("sigint flood", False, f"ready plants missing in {text!r}")
            return [run]
        leader, sibling = int(leaders[1]), int(siblings[1])
        planted.extend([leader, sibling])
        lead, sib = wait_planted(leader, sibling, run)
        shape = adversarial(lead, sib, run, session.proc.pid)
        if shape or not proc_alive(engine):
            record(
                "sigint flood",
                False,
                "ready plant was not the attack: " + "; ".join(shape or ["engine already dead"]),
            )
            return [run]
        threads = []
        import threading

        for _ in range(4):
            thread = threading.Thread(target=flood, args=(session.proc.pid, 1.0))
            thread.start()
            threads.append(thread)
        for thread in threads:
            thread.join()
        deadline = time.time() + 5
        while session.proc.poll() is None and time.time() < deadline:
            session.read_for(0.1)
        if session.proc.poll() is None:
            session.kill()
        status = session.proc.returncode
        text = session.text()
        paths = evidence_dirs(text)
        lines = [line for line in text.splitlines() if line.strip()]
        last = lines[-1] if lines else ""
        survivors = [pid for pid in (engine, leader, sibling, window) if proc_alive(pid)]
        touching = pids_touching(run)
        copied = False
        if paths:
            marker = os.path.join(paths[-1], "launch-1", "verify", "hold", "stdout.txt")
            copied = os.path.isfile(marker) and open(marker, encoding="utf-8").read() == "hold\n"
        ok = (
            status == 130
            and last.startswith("evidence: ")
            and verdict(status, text)
            and not survivors
            and not touching
            and copied
            and not os.path.isdir(run)
        )
        record(
            "sigint flood",
            ok,
            f"status {status} last {last!r} survivors {survivors} touching {touching} copied {copied} run-left {os.path.isdir(run)}",
        )
    finally:
        session.kill()
        session.close()
        kill_pids(planted)
        if run:
            kill_pids(pids_touching(run))
            subprocess.run(["rm", "-rf", run], check=False)
    return paths


def main():
    paths = []
    paths.extend(run_marker())
    paths.extend(run_readonly())
    paths.extend(run_hold())
    cleanup_paths(paths)
    if failures:
        say("FAILED " + " ".join(failures))
        raise SystemExit(1)
    say("drive checks passed")


main()
PY
