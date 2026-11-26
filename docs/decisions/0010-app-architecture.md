# 0010: One service, two front doors (desktop app and local browser mode)

**Status:** Accepted (2026-11-26)

## Context
The owner wants a desktop app with an interactive graph. The UI must be testable
automatically, and people who cannot install the desktop app should still be able to use
the tool without any server or cost.

## Decision
- `ct-service` holds every operation the UI needs (`AppService`), one method per command,
  with request/response types in `views.rs`. `app/ui/src/api/types.ts` mirrors them.
- Two thin front doors call the same methods with the same command names and shapes:
  - **Desktop app** (`app/src-tauri`, Tauri 2): `invoke(<command>, { req })`, plus
    `trace_progress` events.
  - **Local browser mode** (`chaintrail serve`): `POST /api/<command>` on 127.0.0.1 only,
    with a random per-run token in the `x-chaintrail-token` header so other websites cannot
    use it. It also serves the built UI.
- The UI detects which one it runs in; a fixture-backed mock mode (`?mock=1`) allows demos
  and UI tests without network.

## Consequences
- The UI is developed and tested in a normal browser (Vite + Playwright) against real data.
- Behaviour cannot drift between desktop and browser: both call `AppService`.
- `app/src-tauri` is its own Cargo workspace, so core CI does not need GUI libraries.
