"""Non-publishing environments for release tests, including deliberately broken mutants."""
from contextlib import contextmanager
import os
from pathlib import Path
import tempfile


@contextmanager
def isolated_release_environment():
    # Build an allowlist instead of inheriting Git settings, Actions contexts,
    # credentials, Python startup hooks, or an ambient PATH. Git/gh remain inert
    # even when mutation testing removes the publisher's event/version guards.
    with tempfile.TemporaryDirectory(prefix="release-test-tools-") as directory:
        tools = Path(directory)
        for name in ["git", "gh"]:
            executable = tools / name
            executable.write_text("#!/bin/sh\nprintf 'blocked unmocked release tool\\n' >&2\nexit 97\n")
            executable.chmod(0o755)
        yield {
            "PATH": str(tools) + os.pathsep + os.defpath,
            "GITHUB_EVENT_NAME": "workflow_dispatch",
            "GITHUB_REF": "refs/heads/test-only",
            "GITHUB_SHA": "0" * 40,
            "GITHUB_REPOSITORY": "invalid/never-publish",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
