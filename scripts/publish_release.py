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
