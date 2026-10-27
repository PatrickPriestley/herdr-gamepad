import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("publish_release", ROOT / "scripts/publish_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.root.joinpath("Cargo.toml").write_text('[package]\nversion="1.2.3"\n')
        self.root.joinpath("herdr-plugin.toml").write_text('version="1.2.3"\n')
        self.root.joinpath("Cargo.lock").write_text('[[package]]\nname="dependency"\nversion="9.0.0"\n[[package]]\nname="herd-gamepad"\nversion="1.2.3"\n')
        self.env = dict(GITHUB_EVENT_NAME="push", GITHUB_REF="refs/tags/v1.2.3", GITHUB_SHA="a" * 40, GITHUB_REPOSITORY="owner/repo")
        self.run = Mock(side_effect=self.respond)
        self.draft = False
        self.existing = False
        self.api_status = "404"
        self.remote = "a" * 40 + "\trefs/tags/v1.2.3\n"
        self.failure_command = None

    def respond(self, argv, **kwargs):
        if argv[:2] == ["git", "rev-parse"]:
            output = "a" * 40 + "\n"
        elif argv[:2] == ["git", "ls-remote"]:
            output = self.remote
        elif argv[:2] == ["gh", "api"]:
            output = json.dumps({"draft": self.draft, "tag_name": "v1.2.3"}) if self.existing else json.dumps({"status": self.api_status})
            return subprocess.CompletedProcess(argv, 0 if self.existing else 1, output, "API failure")
        else:
            output = ""
        if self.failure_command and argv[:len(self.failure_command)] == self.failure_command:
            result = subprocess.CompletedProcess(argv, 1, output, "failed")
            if kwargs.get("check"):
                result.check_returncode()
            return result
        return subprocess.CompletedProcess(argv, 0, output, "")

    def publish(self):
        return release.publish(self.root, self.env, self.run)

    def test_success_creates_verified_source_release(self):
        self.assertEqual(self.publish(), "published v1.2.3")
        self.assertEqual(self.run.call_args.args[0], ["gh", "release", "create", "v1.2.3", "--repo", "owner/repo", "--verify-tag", "--generate-notes"])
        self.assertIn(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], [c.args[0] for c in self.run.call_args_list])

    def test_annotated_tag_uses_peeled_commit(self):
        self.remote = "b" * 40 + "\trefs/tags/v1.2.3\n" + "a" * 40 + "\trefs/tags/v1.2.3^{}\n"
        self.assertEqual(self.publish(), "published v1.2.3")

    def test_events_refs_and_stable_tag_format(self):
        self.assertEqual(release.release_tag("push", "refs/tags/v0.10.20", ["0.10.20"] * 3), "v0.10.20")
        for event, ref in [("pull_request", "refs/tags/v1.2.3"), ("workflow_dispatch", "refs/tags/v1.2.3"), ("push", "refs/heads/v1.2.3")]:
            with self.subTest(event=event, ref=ref), self.assertRaisesRegex(ValueError, "tag push"):
                release.release_tag(event, ref, ["1.2.3"] * 3)
        for tag in ["1.2.3", "v01.2.3", "v1.02.3", "v1.2.03", "v1.2", "v1.2.3-rc.1", "v1.2.3+build", "v1.2.3\n", "v-1.2.3", "v1.x.3"]:
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "stable"):
                release.release_tag("push", "refs/tags/" + tag, [tag[1:]] * 3)

    def test_all_three_version_files_must_match(self):
        for versions in [["9", "1.2.3", "1.2.3"], ["1.2.3", "9", "1.2.3"], ["1.2.3", "1.2.3", "9"], ["1.2.3"] * 2, ["1.2.3"] * 4]:
            with self.subTest(versions=versions), self.assertRaisesRegex(ValueError, "versions"):
                release.release_tag("push", "refs/tags/v1.2.3", versions)
        for filename in ["Cargo.toml", "Cargo.lock", "herdr-plugin.toml"]:
            path = self.root / filename
            original = path.read_text()
            path.write_text(original.replace("1.2.3", "1.2.4"))
            with self.subTest(file=filename), self.assertRaisesRegex(ValueError, "versions"):
                self.publish()
            path.write_text(original)

    def test_checkout_and_remote_tag_identity(self):
        self.env["GITHUB_SHA"] = "b" * 40
        with self.assertRaisesRegex(ValueError, "checkout"):
            self.publish()
        self.env["GITHUB_SHA"] = "a" * 40
        for remote in ["", "b" * 40 + "\trefs/tags/v1.2.3\n"]:
            self.remote = remote
            with self.assertRaisesRegex(ValueError, "remote tag"):
                self.publish()

    def test_ancestry_and_creation_failures_propagate(self):
        for command in [["git", "rev-parse"], ["git", "merge-base"], ["gh", "release", "create"], ["git", "ls-remote"]]:
            self.failure_command = command
            with self.subTest(command=command), self.assertRaises(subprocess.CalledProcessError):
                self.publish()

    def test_existing_published_release_is_noop_but_draft_is_error(self):
        self.existing = True
        self.assertEqual(self.publish(), "already published v1.2.3")
        self.assertEqual(self.run.call_args.args[0][:2], ["gh", "api"])
        self.draft = True
        with self.assertRaisesRegex(ValueError, "draft"):
            self.publish()

    def test_only_explicit_not_found_allows_create(self):
        for status in ["401", "403", "429", "500", None, 404]:
            self.api_status = status
            with self.subTest(status=status), self.assertRaises(subprocess.CalledProcessError):
                self.publish()

    def test_cli_refuses_non_tag_events(self):
        env = dict(os.environ, GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_REF="refs/tags/v0.1.0")
        result = subprocess.run([sys.executable, str(ROOT / "scripts/publish_release.py")], env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tag push", result.stderr)

    def test_workflow_gates_and_permissions(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("  publish:\n", text)
        before, publish = text.split("\n  publish:\n", 1)
        self.assertIn("permissions:\n  contents: read", before)
        self.assertIn("    needs: macos\n", publish)
        self.assertIn("if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')", publish)
        self.assertIn("    permissions:\n      contents: write", publish)
        self.assertIn("ref: ${{ github.sha }}", publish)
        self.assertIn("fetch-depth: 0", publish)
        self.assertIn("cancel-in-progress: false", publish)
        self.assertIn("GH_TOKEN: ${{ github.token }}", publish)
        self.assertIn("run: python3 scripts/publish_release.py", publish)
        self.assertNotIn("always()", publish)
        self.assertNotIn("continue-on-error", text)
        self.assertIn("python3 -m unittest discover -s tests -p 'test_release.py'", before)


if __name__ == "__main__":
    unittest.main()
