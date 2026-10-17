"""The driver patch must be applied with core.autocrlf=false, or a Windows build is not byte-identical.

Git for Windows ships core.autocrlf=true, and `git apply` then writes CRLF into the files it patches
(measured on a host: 127 of 419 Rust files and contract/manifest.json). The build scripts therefore pass
`-c core.autocrlf=false` to every `git apply` of cua-driver.patch. This checks that each one does, and
shows on any OS what the flag changes.
"""
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def git(cwd, *args, env=None):
    return subprocess.run(["git", *args], cwd=cwd, env=env, capture_output=True, text=True, check=True)


class ApplyEndings(unittest.TestCase):
    def test_every_apply_of_the_driver_patch_turns_autocrlf_off(self):
        sh = (ROOT / "impl/native/apply-and-build.sh").read_text()
        applies = [line for line in sh.splitlines() if re.search(r"\bgit\b.*\bapply\b", line) and "PATCH" in line]
        self.assertGreaterEqual(len(applies), 3, applies)
        for line in applies:
            self.assertIn("core.autocrlf=false", line, line)
        ps = (ROOT / "app/tests/windows/build.ps1").read_text()
        applies = [line for line in ps.splitlines() if "'apply'" in line]
        self.assertEqual(len(applies), 2, applies)
        for line in applies:
            self.assertIn("core.autocrlf=false", line, line)

    def test_the_flag_is_what_keeps_lf(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "r"
            repo.mkdir()
            global_config = Path(tmp) / "gitconfig"
            global_config.write_text("[core]\n\tautocrlf = true\n")
            env = {**os.environ, "GIT_CONFIG_GLOBAL": str(global_config), "GIT_CONFIG_SYSTEM": os.devnull}
            git(repo, "init", "-q", env=env)
            (repo / "a.rs").write_bytes(b"one\ntwo\n")
            patch = (
                b"diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,3 @@\n one\n+added\n two\n"
            )
            (Path(tmp) / "p.patch").write_bytes(patch)
            outcomes = {}
            for name, prefix in (("default", []), ("off", ["-c", "core.autocrlf=false"])):
                (repo / "a.rs").write_bytes(b"one\ntwo\n")
                git(repo, *prefix, "apply", str(Path(tmp) / "p.patch"), env=env)
                outcomes[name] = (repo / "a.rs").read_bytes()
            # the control: with the host's default the patched file gets CRLF
            self.assertIn(b"\r\n", outcomes["default"])
            self.assertEqual(outcomes["off"], b"one\nadded\ntwo\n")


if __name__ == "__main__":
    unittest.main()
