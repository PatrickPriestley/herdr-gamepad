# Clear unsent dictation with the controller

User request: remove an unwanted dictated message entirely; defer replacement recording.

## Probed behavior and boundary

Native dictation is normalized to a single logical line, including repeated appended recordings. Codex 0.165.0 default keymap and textarea implementation bind Ctrl+A to logical-line start and Ctrl+K to kill-to-end-of-line. Claude Code official interactive-mode text-editing documentation describes the same sequence. This clears the entire normal dictated message from any caret position, even if it visually wraps. It is harmless on an empty single-line composer. Actual keyboard-created multiline drafts are a stated limitation: only the current logical line clears.

Do not use Ctrl+C (can interrupt/exit on empty input), Escape (can dismiss/rewind), or Claude Ctrl+S (stashes and can restore old input). Support Codex and Claude with their standard editor shortcuts; other agents get a clear unsupported notice.

## Implementation

- Square/X clears the input line while native submission is staged, and in Respond mode after its existing pane check. Ordinary Navigate Square remains zoom; non-agent Interact Square remains Space. Menus retain their existing controls.
- Validate original focused pane, same agent kind, supported editor, and eligible status before clearing, then recheck at the Herdr boundary. Never send clear keys to a shell, blocked dialog, or moved target.
- Do not clear while recording/transcribing or on a same-tick queued press after insertion. Consume repeated Square presses until release so clearing cannot fall through to zoom. Reset the latch on menu/disconnect ownership changes.
- Clear disarms staged submission and returns to Navigate. Send both editor shortcuts in one agent send-keys call, with no Enter. Only acknowledge successful clear; on failure do not announce success or retry automatically.
- Update mapping/help and README with the single-line/default-keymap boundary. No automatic replacement, no merges/releases.

## Validation

Assertion-red tests for staged and Respond clear, original target checks, unsupported/blocked states, repeated/busy/fresh presses, Navigate zoom and Interact preservation, bridge argv/validation, and no false success toast on failure. Probe actual CLI key arguments through a fake socket; no input to live agents. Mutation-check new guards, full suite/source build, independent different-family review before push. Install for manual controller testing.

## Implementation evidence

The installed Herdr CLI was run against a temporary fake Unix socket with its real ping handshake. Captured request: `agent.send_keys` with `keys: ["ctrl+a", "ctrl+k"]`; no live pane received input. Seven pure Core tests and the bridge/dispatch regressions failed on assertions before their fixes. 328 unit tests and four integration tests pass, one opt-in live test is ignored; all 24 new guard/dispatch mutations were caught (20 Core, four bridge/dispatch). Source build and whitespace checks pass. Controller/microphone acceptance remains manual.

Independent GLM review completed with `<<AGENT-REVIEW-END>>` and no important correctness/safety findings. It independently ran the suite. Minor accepted notes: a pre-dispatch refusal is worded "Clear unconfirmed"; a dispatched clear returns to Navigate even on failure, so retry through Respond. No automatic replacement or arbitrary multiline clear was added.
