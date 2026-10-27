# Voice delivery independent of controller activity

User report: voice seems not to work until a face button is pressed. This work fixes event-loop starvation only; blocked-agent questions still use explicit controller confirmation.

## Probed facts

- Actual gilrs 0.11.2 `next_event_inner` repeats its blocking wait after each filtered event. The macOS backend applies the supplied timeout per received raw event, not as an overall deadline.
- Read-only probe `/tmp/voice-wait-probe` queued a synthetic finished transcript after 150 ms and used the daemon's blocking 100 ms wait. It failed the 250 ms delivery assertion: a real DPadLeft event arrived after 2.554 s and the transcript was delivered 2.425 s after completion.
- With default filters disabled, the probe exposed LeftStickX noise alternating -0.039/-0.031 every 1–6 ms and delivered in 3.19 ms. This isolates the reset timeout to filtered stick noise.
- Retaining filters and using nonblocking polling plus a 10 ms tick delivered in 10.48 ms. The final production `next_inputs` helper, linked into a read-only hardware probe, delivered in 13.19 ms.
- The four read-only Herdr snapshot commands took 33–39 ms total (7–12 ms each). This alone does not explain multi-second lag. No notifications were emitted during measurement.

## Decision

Poll one available controller event nonblockingly per loop, retaining gilrs's default filters and mappings. Drain completed voice results on every loop. Order already-completed transcripts before a newly-polled button so that a simultaneous push-to-talk press can start the next recording. Add one 10 ms sleep per tick to bound idle CPU usage and avoid starving the transcription worker. This is a synchronous polling seam, not a new thread or channel protocol.

Alternatives rejected: disabling default filters changes controller mappings/dead-zone handling; a dedicated controller thread adds shutdown and ownership complexity to a two-source scheduling problem.

## Validation boundary

Three CI tests exercise the actual collection seam: completed transcript with no button plus the production adapter contract; completed transcript before next voice press; exactly one controller poll and an idle throttle. The adapter contract is textual because a hardware-independent fake cannot prove which gilrs API the real closure invokes. It is paired with the real-library red/green probe above, not offered as standalone runtime proof.

No pure business module changes. Run cargo-mutants on the new helper and manual scheduling mutations; the latter remove throttle, channel drain, controller poll, transcript ordering, and nonblocking adapter individually. Real speech/transcription roundtrip remains a user smoke test after installing the build.
