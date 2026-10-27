---
change_id: background-operation
status: diverged
accepted_by: PatrickPriestley (instruction to plan and then implement)
accepted_on: 2026-10-27
divergences:
  - Integration and worker-shutdown coverage was added after wiring; scoped guard mutations verified these assertions detect broken shutdown.
  - Use the user's three explicit requirements as the spec and proceed directly to implementation as requested, without a separate spec/plan PR approval round.
---
# Plan: background gamepad operation

## Requirements

- R1: Start runs without a pane; Stop reliably ends that daemon, including active recording/transcription. Repeated/concurrent starts and stops are safe.
- R2: Bounded logs are available through an explicit Show logs action.
- R3: Mapping remains an overlay opened only by Show mapping.

## Evidence and design

A throwaway Herdr 0.9.1 action (`printf ready; sleep 2; printf done`) ran without a pane. Its native log had status running but no output until exit. Therefore Start will run `serve` as a Herdr-owned action (not detach another process), while the daemon writes its own bounded state log. The action retains its installation's lifecycle ownership.

Keep daemon.pid flock as the single-instance authority. Write an instance token under that lock. Stop writes an instance-specific daemon.stop.<token> request, then waits for the matching instance to release its lock. A short control-file flock serializes token publication and stop-request creation. Only the daemon matching the token observes it. No PID signalling; stale requests cannot stop a new instance. Legacy PID-only daemons require a one-time stop of the old Gamepad pane, rather than signalling an unverified PID.

A shared cancellation check reaches the event loop, Herdr subprocess calls and whisper subprocess calls. Stop drops a recording, cancels and joins transcription, discards pending transcripts, and releases the daemon lock. Herdr/whisper subprocess groups are cleaned up by the existing bounded runner.

Log writes are serialized and rotate at 1 MiB, retaining one prior log. Show logs opens a snapshot of the tail in an overlay, including errors if initialization failed. Closing logs or mapping does not affect the daemon. No startup hooks or auto-opening UI.

## Files that change

- New src/lifecycle.rs, src/lifecycle_tests.rs: lock, instance tokens, stop acknowledgement.
- New src/logging.rs, src/logging_tests.rs: bounded rotating log and tail reader.
- src/serve.rs, src/main.rs, src/voice.rs: startup, graceful cancellation and on-demand log UI.
- src/herdr.rs, src/herdr_tests.rs: cancellable subprocess runner.
- src/serve_tests.rs, tests/manifest.rs, new tests/background.rs: lifecycle and UI regressions.
- herdr-plugin.toml, README.md, CHANGELOG.md, docs/plans/2026-10-27-gamepad-design.md: actions, operating instructions, architecture.

## Order of work and tests

1. R1/R3: manifest regression requires Stop and Show logs actions and absence of a daemon pane. Fails against the current daemon pane/start entrypoint.
2. R1: test lock exclusivity, stale stop requests, current-instance acknowledgement, legacy refusal, startup error, and stop timeout; add cancellation runner test that fails today because the command runs to timeout rather than cancellation.
3. R2: test rotation and tail reading against temporary files; implement serialized log writes and explicit log overlay.
4. R1: wire a scoped transcription worker so cancellation finishes before releasing the lock. End-to-end subprocess tests use a hardware-free worker exercising the real lifecycle manager.
5. R1/R2/R3: real Herdr integration: Start does not change pane inventory, repeat Start does not duplicate, Stop removes the worker, explicit logs/mapping create overlays only on request.
6. Run `cargo test --locked`, `sh scripts/build.sh`, `actionlint .github/workflows/ci.yml`, `git diff --check`; mutate lifecycle/cancellation guards in an isolated copy. Independent different-family review of plan and final diff, evaluate findings before installation.

## Risks

- Stop must interrupt subprocess waits and must never deliver a late transcript.
- A stale stop request or reused PID must not affect another process/daemon generation.
- Concurrent Start/Stop can race; the lock and generation token define ownership and acknowledgement.
- Herdr owns the long-running action. Logs cannot rely on its completion-only capture.
- Microphone permission still depends on the terminal/Herdr launch context and needs a live check.

## Alternatives not taken

- launchd or a detached child: introduces separate ownership, environment, and installation-cleanup concerns.
- kill(pidfile): PID reuse can signal an unrelated process.
- Hidden daemon pane: still ties operation to a terminal pane and does not meet R1.
- Unbounded logs: a long-lived service should not grow state indefinitely.


## Verification and independent review

- `cargo test --locked`: 118 unit tests, 2 background integration tests, and 2 manifest tests passed.
- `sh scripts/build.sh`, `actionlint .github/workflows/ci.yml`, and `git diff --check`: passed.
- Fourteen isolated guard mutations were killed: singleton ownership, control serialization,
  stop observation, legacy validation, stop timeout/acknowledgement, pre-spawn/running
  cancellation, rotation/line size, worker cancellation/join, and export preflight/chunk cancellation.
- Live Herdr 0.9.1: Start and repeated Start retained one daemon and unchanged pane inventory;
  Show logs and Show mapping opened only the requested overlays; closing them preserved the
  daemon; Stop/repeated Stop exited cleanly; restart produced a new instance. The old v0.1.0
  Gamepad pane was closed and the local development plugin linked, preserving state/config.
- GLM 5.3 Flash via OpenCode Go independently reviewed the implementation and ran tests.
  Findings about token publication and Stop probe races described the earlier implementation:
  the final `daemon.control` lock serializes both paths. The review's claim that production
  never references daemon.control is contradicted by `control()` and its callers; removing
  the Start control lock makes the serialization test fail. A token change after the original
  instance exits intentionally acknowledges that instance, without stopping its replacement.
  The requested real-Herdr integration was completed as recorded above.
- Accepted review finding: WAV export could delay cancellation before whisper starts. Added
  a failing regression, then cancellation before file creation and between 1024-sample chunks;
  both guards were independently mutation-tested.
- Fresh controller/microphone interaction after this lifecycle change still requires human
  hardware validation. Automated tests cover cancellation and Herdr lifecycle/UI integration.
