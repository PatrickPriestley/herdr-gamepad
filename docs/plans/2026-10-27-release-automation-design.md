# Release automation

Accepted request: automate publishing after a version tag passes CI, so future releases
only require pushing the tag. No version bump or release publication is part of this change.

Use a publishing job in the existing CI workflow, dependent on the macOS matrix. Only
pushes of version tags can run it. Keep the existing protected-branch check names and
read-only default token; only the release job receives contents: write. Pin checkout to
the triggering SHA, fetch history, and serialize publishing per tag without cancellation.

A Python standard-library helper validates a stable vMAJOR.MINOR.PATCH tag against
Cargo.toml, Cargo.lock's herd-gamepad entry, and herdr-plugin.toml. It verifies that the
checked-out commit is on origin/main and the remote tag still resolves to that commit.
It creates a source release with generated notes and --verify-tag. An already published
release is a successful no-op. An existing draft or an API failure fails visibly; only
an explicit 404 means a release should be created. No prereleases or binary uploads.

Probed facts:
- Current workflow runs on main, PRs and v* tags but has no publishing job.
- Installed gh release create help confirms --verify-tag and --generate-notes.
- Read-only gh api on v0.1.0 returned draft:false and tag_name:v0.1.0; a nonexistent
  version returned exit1 and JSON status:"404". This distinguishes absent from API errors.
- Local Python3 supports tomllib. GitHub runner-images documents Python3.14 on macos-15
  and Python3.12 on Ubuntu24.04; helper requires Python3.11+.
- GitHub's workflow syntax says needs requires successful dependencies; contents:write
  permits release creation. Use this direct dependency, not a separate workflow_run trigger.

Alternatives: manual gh publishing (current friction); automatic releases on every main
merge (does not preserve the user's explicit version-tag choice); cross-workflow publish
adds an unnecessary trigger/permission boundary.

Tests will use temporary metadata and stubbed subprocesses, plus a local Git repository
probe, never create a real release. They cover success, event/tag/version guards, missing
or duplicate lock entries, ancestry/tag identity, existing release/draft, API failures,
and create failures. Mutate the helper's guards before committing the implementation plan.
README will document version-bump PR, annotated tag push, automatic publication and retry.
