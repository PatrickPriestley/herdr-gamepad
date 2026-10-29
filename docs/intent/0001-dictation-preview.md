---
change_id: 0001-dictation-preview
author: Patrick Priestley
created: 2026-10-29
status: draft
tracker: null
size: null
---
# Intent: Optional dictation preview

## Problem

The user wants to read speech-to-text before sending it: “I want to be able to see and then I should also be able to delete and retranscribe.” Current dictation submits a completed transcript automatically, leaving no opportunity to discard or replace an incorrect transcription. This request is the user-reported evidence; no separate incident or measurement was supplied.

## Proposed outcome

People can turn preview before sending on or off. Preview shows the full transcript and its destination, lets the user send it deliberately, delete the draft, or record a replacement. Back retains the draft for review later. A failed replacement or send preserves the text. Preview is optional: automatic sending stays the default and remains available.

## Affected users and systems

People dictating messages to agents through the gamepad plugin, using PlayStation, Xbox, or combined button labels. The affected experience includes dictation, controller interaction, preview presentation, settings, and agent message delivery.

## Constraints

Cross/A sends once; Square/X deletes the entire draft; holding the configured voice button records again; Circle/B returns without sending. Re-recording means new speech, not rerunning the same audio. Changing focus, hiding the preview, or turning preview off must never send a pending draft automatically or redirect it to another pane. Joystick scrolling, word-level editing, and release publication are separate work.

## Open questions

The spec must define draft lifetime across disconnect/restart, what happens when a new recording starts with a hidden draft, and recovery when delivery might have succeeded but acknowledgement was lost. These are engineering decisions to propose explicitly; retaining text must not imply automatically resending it. The user accepted the feature direction in conversation and asked to continue with feature one; formal acceptance of this intent is its merge into main.
