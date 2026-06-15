# VRCRecorder — Audit Baseline

This folder is the **baseline reference** for the pre-release audit rounds of VRCRecorder
(target: publishing on stores such as [booth.pm](https://booth.pm)). It was produced by
reading the full source tree as of the current working copy, before any audit fixes.

Audit focus areas requested:

1. **Functional correctness** — does it record what it claims, A/V in sync, no data loss.
2. **Performance** — CPU/GPU/RAM cost, frame drops, scalability to long recordings.
3. **Interface / UX** — does the UI work, is it understandable, store-readiness.

## Documents

| File | Purpose |
|------|---------|
| [`01-architecture.md`](01-architecture.md) | System overview: threads, data flow, external dependencies, file layout. The mental model. |
| [`02-functional-spec.md`](02-functional-spec.md) | Behavior inventory — the *intended* behavior of every feature, config field, IPC command, and integration. The "ground truth" the audit checks against. |
| [`03-audit-plan.md`](03-audit-plan.md) | Audit method + per-dimension checklists and concrete test scenarios. |
| [`04-observations.md`](04-observations.md) | Pre-audit risk register: candidate issues found while reading the code, with `file:line` refs, severity, and confidence. The starting backlog. |

## How to use this

- Treat `02-functional-spec.md` as the contract. When auditing, compare observed behavior to
  the documented intent and record any divergence.
- `04-observations.md` already lists likely problems. **Confirmed-from-code** items can go
  almost straight to a fix backlog; **risk** items need runtime verification first.
- Each audit round should append results (pass/fail + evidence) rather than editing the
  baseline, so the baseline stays a stable reference point.

## Snapshot context (verify before starting)

- **Version**: `0.2.0` (`Cargo.toml`, `tauri.conf.json`).
- **Working copy is dirty**: ~10 tracked files modified and uncommitted (notably a
  `cpal 0.15 → 0.18` + `rfd 0.15 → 0.17` dependency bump and an in-progress *SteamVR
  auto-launch* feature). **Reconcile/commit these before auditing** so the baseline is stable.
- **No automated tests exist** in the repo (`grep` for `#[test]` finds none). All verification
  is currently manual.
- **Platform**: Windows only (WASAPI loopback, SteamVR, `creation_flags`, `SetDllDirectoryW`).
