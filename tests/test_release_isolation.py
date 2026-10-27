"""Adversarial integration regression: mutation tests must never inherit publish access."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ReleaseIsolationTests(unittest.TestCase):
    def test_mutation_run_cannot_publish_from_inherited_context(self):
        with tempfile.TemporaryDirectory(prefix="release-isolation-") as directory:
            root = Path(directory)
            repo = root / "context"
            repo.mkdir()
            # Fixture Git uses no ambient Git settings, credentials, hooks or remote URLs.
            fixture_env = {"PATH": os.defpath, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}
            git_binary = shutil.which("git", path=os.defpath)
            self.assertIsNotNone(git_binary)
            def git(*args):
                return subprocess.check_output([git_binary, "-C", str(repo), *args], env=fixture_env, text=True, stderr=subprocess.PIPE).strip()
            git("init", "-q", "-b", "main")
            for name in ["Cargo.toml", "Cargo.lock", "herdr-plugin.toml"]:
                shutil.copy2(ROOT / name, repo / name)
            git("add", ".")
            git("-c", "user.name=Probe", "-c", "user.email=probe@example.invalid", "commit", "-qm", "fixture")
            sha = git("rev-parse", "HEAD")
            version = tomllib.loads((repo / "Cargo.toml").read_text())["package"]["version"]
            tag = "v" + version
            git("tag", tag)
            subprocess.run([git_binary, "clone", "--bare", "-q", str(repo), str(root / "remote")], env=fixture_env, check=True, capture_output=True)
            git("remote", "add", "origin", str(root / "remote"))
            git("fetch", "-q", "origin")
            copied = root / "under-test"
            copied.mkdir()
            for folder in ["scripts", "tests", ".github"]:
                shutil.copytree(ROOT / folder, copied / folder, ignore=shutil.ignore_patterns("__pycache__"))
            for name in ["Cargo.toml", "Cargo.lock", "herdr-plugin.toml"]:
                shutil.copy2(ROOT / name, copied / name)
            tools = root / "tools"
            tools.mkdir()
            marker = root / "attempted-release.jsonl"
            stub = tools / "gh"
            stub.write_text(f'''#!{sys.executable}
import json, sys
from pathlib import Path
if sys.argv[1] == "api":
    print(json.dumps({{"status": "404"}}))
    sys.exit(1)
with Path({str(marker)!r}).open("a") as output:
    output.write(json.dumps(sys.argv[1:]) + "\\n")
''')
            stub.chmod(0o755)
            # Deliberately hostile but credential-free: only the local bare remote and
            # recording gh stub are reachable, even against the unfixed implementation.
            env = dict(fixture_env, PATH=str(tools) + os.pathsep + os.defpath,
                       GIT_DIR=str(repo / ".git"), GIT_WORK_TREE=str(repo),
                       GITHUB_EVENT_NAME="push", GITHUB_REF="refs/tags/" + tag,
                       GITHUB_SHA=sha, GITHUB_REPOSITORY="probe/repo")
            control = subprocess.run([sys.executable, "scripts/publish_release.py"], cwd=copied, env=env,
                                     capture_output=True, text=True, timeout=15)
            self.assertEqual(control.returncode, 0, control.stdout + control.stderr)
            self.assertEqual(json.loads(marker.read_text().splitlines()[0])[:3], ["release", "create", tag])
            marker.unlink()
            result = subprocess.run([sys.executable, "tests/mutate_release.py"], cwd=copied, env=env,
                                    capture_output=True, text=True, timeout=60)
            calls = [json.loads(line) for line in marker.read_text().splitlines()] if marker.exists() else []
            self.assertEqual((result.returncode, calls), (0, []), result.stdout + result.stderr)
            self.assertIn("All 33 release mutations caught", result.stdout)


if __name__ == "__main__":
    unittest.main()
