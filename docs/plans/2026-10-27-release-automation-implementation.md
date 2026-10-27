# Tag-driven Releases Implementation Plan

> Execute through feature-loop's subagent-driven development, with spec then quality review.

**Goal:** A stable version tag publishes a GitHub source release only after both macOS CI jobs pass.
**Architecture:** A dependent CI publishing job with narrowly scoped write permission calls a Python3.11+ helper. Version metadata, main ancestry, and remote tag identity are validated before any publishing command. Explicit API404 permits creation; published releases are idempotent; drafts/errors fail.
**Tech Stack:** GitHub Actions, git, gh, Python standard library unittest/tomllib.

## Task group: publisher, workflow, tests, documentation

Files: scripts/publish_release.py, tests/test_release.py, tests/mutate_release.py,
.github/workflows/ci.yml, .gitignore, README.md, CHANGELOG.md.

1. Add the tests below and run `python3 -m unittest discover -s tests -p test_release.py`.
   It must fail because the publishing module is absent. Add the helper and verify
   the workflow test still fails on the current workflow (no publish job).
2. Apply the assembled helper and workflow below; run the 10 tests and actionlint.
   The exact code below already passed in /tmp/herdr-release-plan.
3. Run `python3 tests/mutate_release.py`. All33 helper mutations must fail tests.
   Remove workflow dependency, event condition, read-only default, pinned ref, test
   invocation and noncancelling concurrency in isolated copies; each must fail tests.
4. Add __pycache__/ to .gitignore. README: after merging version changes in all three
   TOML files and updating CHANGELOG, create/push an annotated stable vX.Y.Z tag from
   updated main. CI then publishes generated notes automatically. No manual gh release
   create is needed. Only stable tags supported. Existing published releases are no-ops;
   draft releases require manual handling. On transient failure rerun the tag workflow;
   on version/content errors fix via PR and use a new correct version tag, do not move
   a published tag. Installation remains source-built. Add Unreleased changelog entry.
5. Run Python tests and mutations, cargo test --locked, sh tests/build.sh, actionlint,
   and git diff --check. Commit tests and implementation separately. No real release/tag
   creation, merging, or daemon restarts are authorized by this implementation task.
6. Fresh spec review then quality review; final different-family herd-review. Fix
   verified findings test-first and re-review. Open a PR against main.

## Probes and plan verification

- Design records CLI/API probes and primary GitHub workflow/runner documentation.
- Absent module failed import, then the old workflow failed its positive publish-job assertion.
- Assembled code passes 10 tests; Python compilation and actionlint pass.
- All33 publisher guard/return/subprocess/regex mutations caught, zero survivors.
- Six workflow guard mutations caught. Unconditional-comprehension true filters were
  excluded as equivalent (they add no guard); real filtering predicates are mutated.
- Unchecked git rev-parse initially survived; a failure-path assertion now catches it.
- A temporary local Git repository and bare remote exercised actual annotated tag
  peeling, main ancestry, CLI environment/error status and fake gh creation. Retargeting
  that local tag caused nonzero exit with 'remote tag no longer points to the tested commit'.
  No GitHub release was created; only a stubbed gh was used for publish-path execution.

## Exact assembled files

### scripts/publish_release.py

```python
#!/usr/bin/env python3
"""Publish a tested stable source release; requires Python 3.11+ and gh."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


def release_tag(event, ref, versions):
    if event != "push" or not ref.startswith("refs/tags/"):
        raise ValueError("publishing requires a tag push")
    tag = ref.removeprefix("refs/tags/")
    if re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag) is None:
        raise ValueError("release tag must be a stable vMAJOR.MINOR.PATCH version")
    if versions != [tag[1:]] * 3:
        raise ValueError("tag and Cargo.toml, Cargo.lock, plugin versions must match")
    return tag


def publish(root, env, run=subprocess.run):
    cargo = tomllib.loads((root / "Cargo.toml").read_text())
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    plugin = tomllib.loads((root / "herdr-plugin.toml").read_text())
    versions = [cargo["package"]["version"], plugin["version"]]
    versions += [p["version"] for p in lock["package"] if p["name"] == "herd-gamepad"]
    tag = release_tag(env["GITHUB_EVENT_NAME"], env["GITHUB_REF"], versions)
    options = dict(cwd=root, text=True, capture_output=True)
    head = run(["git", "rev-parse", "HEAD"], check=True, **options).stdout.strip()
    if head != env["GITHUB_SHA"]:
        raise ValueError("checkout does not match the tested commit")
    run(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"], check=True, **options)
    remote = run(["git", "ls-remote", "origin", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"], check=True, **options)
    refs = {ref: sha for sha, ref in (line.split() for line in remote.stdout.splitlines())}
    if refs.get(f"refs/tags/{tag}^{{}}", refs.get(f"refs/tags/{tag}")) != head:
        raise ValueError("remote tag no longer points to the tested commit")
    repo = env["GITHUB_REPOSITORY"]
    existing = run(["gh", "api", f"repos/{repo}/releases/tags/{tag}"], **options)
    body = json.loads(existing.stdout)
    if existing.returncode == 0:
        if body["draft"]:
            raise ValueError("an existing draft needs manual review; refusing to publish it")
        return f"already published {tag}"
    if body.get("status") != "404":
        existing.check_returncode()
    run(["gh", "release", "create", tag, "--repo", repo, "--verify-tag", "--generate-notes"], check=True, **options)
    return f"published {tag}"


if __name__ == "__main__":
    try:
        print(publish(Path(__file__).resolve().parents[1], os.environ))
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"release failed: {error}", file=sys.stderr)
        sys.exit(1)
```

### tests/test_release.py

```python
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
```

### tests/mutate_release.py

```python
"""Mutation-check release guards in an isolated copy, using only Python's standard library."""
import ast
import copy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "scripts/publish_release.py").read_text()
tree = ast.parse(source)
mutations = []
for index, node in enumerate(ast.walk(tree)):
    if isinstance(node, ast.If):
        for value in [True, False]:
            mutations.append((index, "test", ast.Constant(value), f"line {node.lineno}: guard={value}"))
    if isinstance(node, ast.comprehension) and node.ifs:
        for value in [True, False]:
            mutations.append((index, "ifs", [ast.Constant(value)], f"comprehension {index}: filter={value}"))
    if isinstance(node, ast.Return):
        mutations.append((index, "value", ast.Constant(None), f"line {node.lineno}: return None"))
    if isinstance(node, ast.keyword) and node.arg == "check":
        mutations.append((index, "value", ast.Constant(False), f"line {node.value.lineno}: unchecked subprocess"))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "exit":
        mutations.append((index, "args", [ast.Constant(0)], f"line {node.lineno}: successful error exit"))
    if isinstance(node, ast.BoolOp):
        mutations.append((index, "op", ast.And() if isinstance(node.op, ast.Or) else ast.Or(), f"line {node.lineno}: boolean operator"))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "fullmatch":
        func = copy.deepcopy(node.func); func.attr = "search"
        mutations.append((index, "func", func, f"line {node.lineno}: regex search"))
    if isinstance(node, ast.Constant) and isinstance(node.value, str) and "[1-9][0-9]*" in node.value:
        for part in range(3):
            bits = node.value.split("(0|[1-9][0-9]*)")
            pattern = ""
            for n, bit in enumerate(bits):
                pattern += bit
                if n < 3:
                    pattern += "[0-9]+" if n == part else "(0|[1-9][0-9]*)"
            mutations.append((index, "value", pattern, f"line {node.lineno}: leading zero component {part}"))

with tempfile.TemporaryDirectory(prefix="release-mutants-") as temporary:
    root = Path(temporary)
    for folder in ["scripts", "tests", ".github"]:
        shutil.copytree(ROOT / folder, root / folder, ignore=shutil.ignore_patterns("__pycache__"))
    for file in ["Cargo.toml", "Cargo.lock", "herdr-plugin.toml"]:
        shutil.copy2(ROOT / file, root / file)
    def run():
        return subprocess.run([sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests", "-p", "test_release.py"], cwd=root, capture_output=True, text=True, timeout=15)
    baseline = run()
    if baseline.returncode:
        sys.exit(baseline.stdout + baseline.stderr)
    survived = []
    for index, field, value, name in mutations:
        changed = copy.deepcopy(tree)
        setattr(list(ast.walk(changed))[index], field, copy.deepcopy(value))
        path = root / "scripts/publish_release.py"
        path.write_text(ast.unparse(ast.fix_missing_locations(changed)))
        result = run()
        if result.returncode == 0:
            survived.append(name)
        print(("SURVIVED " if result.returncode == 0 else "caught ") + name)
    if survived:
        sys.exit(f"{len(survived)} surviving mutations: {survived}")
    print(f"All {len(mutations)} release mutations caught")
```

### .github/workflows/ci.yml

```yaml
name: CI

on:
  push:
    branches: [main]
    tags: ['v*']
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  macos:
    name: Test and build (${{ matrix.runner }})
    strategy:
      fail-fast: false
      matrix:
        runner: [macos-15, macos-15-intel]
    runs-on: ${{ matrix.runner }}
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6
        with:
          persist-credentials: false
      - name: Install stable Rust
        run: |
          rustup toolchain install stable --profile minimal
          rustup default stable
      - name: Test
        run: |
          python3 -m unittest discover -s tests -p 'test_release.py'
          cargo test --locked
      - name: Build through the plugin entrypoint
        run: sh tests/build.sh && sh scripts/build.sh
      - name: Smoke test the installed layout
        env:
          HERDR_PLUGIN_CONFIG_DIR: ${{ runner.temp }}/gamepad-config
          HERDR_PLUGIN_STATE_DIR: ${{ runner.temp }}/gamepad-state
        run: |
          ./bin/herd-gamepad version
          ./bin/herd-gamepad mapping < /dev/null

  publish:
    name: Publish release
    needs: macos
    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    permissions:
      contents: write
    concurrency:
      group: release-${{ github.ref }}
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6
        with:
          ref: ${{ github.sha }}
          fetch-depth: 0
          persist-credentials: false
      - name: Publish verified source release
        env:
          GH_TOKEN: ${{ github.token }}
        run: python3 scripts/publish_release.py
```

## Review correction: isolate release test processes

Independent quality review reproduced a test-harness escape from the original plan: inherited Git and GitHub Actions variables allowed mutated publisher code to reach release creation. A regression first established a positive control using a temporary Git repository, local bare origin, and recording `gh` stub. Before the fix, its assertion failed with three recorded release-create calls and a surviving `__main__` mutation. No GitHub release was created.

The implementation now gives import, CLI, and mutation execution a fresh allowlisted environment with a non-publishing event context and inert `git` and `gh` executables first on PATH. The functional publisher tests retain their injected fake runner. An explicit import assertion catches the removed main guard. The mutation runner rejects syntax, loader, and infrastructure failures as evidence while accepting failures raised within running tests. CI discovers the adversarial regression; mutation children run only ordinary publisher tests to avoid recursion.

This intentionally diverges from the assembled test code above to address the verified review finding. The production publisher and publishing conditions are unchanged. Validation: 12 Python tests, including the hostile inherited-environment regression, and all 33 publisher guard mutations pass; actionlint and whitespace checks pass.
