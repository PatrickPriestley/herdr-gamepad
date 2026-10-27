# Automatic dictation submission

User decision: “just submit automatically.”

Dictation currently submits automatically to Idle/Done agents but types without Enter
for Working agents. Use the same Action::Prompt submission for all three states.
Continue dropping blank transcripts, changed-focus targets, non-agent panes, and
Blocked/Unknown agents. Do not submit into approval dialogs. No extra confirm press,
new mode, configuration option, or separate Enter command is needed.

Probed facts:
- Installed `herdr agent prompt --help` describes acceptance from a non-working or
  working state; it rejects already-blocked agents before sending input.
- Installed `herdr pane send-text --help` describes literal text without Enter.
- `cargo mutants --version` reports 27.1.0; its help supports file-scoped mutation runs.
- The existing exact Working transcript test expects SendText plus “Typed, not sent”.
  Changing it to Prompt plus “Sent” provides a positive regression assertion.

The same Prompt action already covers Idle/Done. Its adapter invokes Herdr's atomic
prompt submission; existing application tests cover refusing delivery and suppressing
a false Sent toast. Update the displayed mapping and README to match automatic sending.

Rejected alternatives: always require manual confirmation (user explicitly declined);
append an unguarded Enter after send-text (unnecessary two-command routing window).

Validation: failing-then-passing Working regression; full suite and release build;
mutation-test src/engine.rs; independent spec, quality, and different-family review.
A live test must target a disposable agent rather than submitting a surprise prompt to
an existing user agent. Controller hardware itself is unchanged.
