# bridge-ext (P1, not yet implemented)

Track A browser extension. Scope per design doc §9:

- MV3, no `selectionchange` listeners, no auto-capture.
- On request from the panel (local 127.0.0.1 poll with pairing token), read
  `window.getSelection()` and return it; empty selection returns empty with an
  explicit signal — never silent.
- Same-origin `fetch('/api/session.export?sessionId=…')` → ZIP (DEFLATE) →
  JSONL, with a self-contained ZIP reader.
- Anchor recovery: unique match of the selection in parsed events →
  `event`, multiple → `ambiguous`.

The local wiring (random port bound to 127.0.0.1 only, one-time pairing token
pasted in the extension options page) is introduced together with this package
in P1. Per-agent cost is a transcript parser only.
