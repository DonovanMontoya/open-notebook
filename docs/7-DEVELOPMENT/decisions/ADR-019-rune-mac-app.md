# ADR-019: The on-device Mac app is drawn with Rune; iOS stays on Tauri

- **Status**: Accepted
- **Date**: 2026-11
- **Related**: [ADR-002](ADR-002-external-libraries.md), [ADR-011](ADR-011-design-token-system.md)

## Context

The on-device app (`apps/desktop`) is Tauri 2 with a React UI and a TypeScript engine (`packages/engine`) that runs the notebooks on SQLite with the user's own API keys, or talks to an Open Notebook server. It targets macOS first, then iOS.

The Mac app is moving to [Rune](https://github.com/anomalyco/rune), a Rust UI framework that draws everything itself on the GPU (wgpu), with springs for motion, native menus and file dialogs, AVFoundation playback, and a headless mode that renders any state to PNG. Rune has no webview and no JavaScript runtime, so neither the React UI nor the TypeScript engine can run inside it. Rune supports macOS, Windows, Linux and the web; iOS is on its roadmap but not ready.

## Decision

1. The Mac app is a Rust workspace in `apps/mac`: `engine` (`notebook-engine`), a port of `packages/engine` and `packages/api-client`, and `app`, the Rune UI.
2. The Rust engine keeps the TypeScript engine's database schema, id and timestamp formats, prompts, data folder (`~/Library/Application Support/com.donovanmontoya.opennotebook`), `keys.json` and `originals/` folder. Either app opens the other's notebooks, keys and files.
3. iOS stays on Tauri + React + the TypeScript engine until Rune's iOS target is usable.
4. Rune is pinned to a revision of its `dev` branch, which is never rewritten.

## Alternatives considered

- **Keep Tauri on the Mac.** One codebase for both platforms, but the Mac app stays a webview and keeps the workarounds that come with it (`window.confirm()` does nothing in WKWebView, the HTTP plugin can't read local audio files).
- **Embed a JavaScript runtime in the Rune app to run the TypeScript engine.** Keeps one engine, but the engine relies on browser APIs (`fetch` streaming, `Blob`, `DOMParser`, pdf.js) that would each need a native shim. That adds a second runtime for less than a port costs.
- **Move iOS to Rune now.** Rune's iOS text input and shell aren't ready.

## Consequences

- Two engines and two UIs exist until iOS moves. A change to the schema, a prompt or a provider has to land in both `packages/engine` and `apps/mac/engine`. The shared formats in point 2 are the contract between them.
- Settings that lived in the webview's localStorage (the chosen models, appearance) start from defaults in the Rune app; keys and data carry over.
- Live voice (WebRTC) isn't in the Rune app yet.
- Screens are checked headlessly (`--snap … --scene …`), and an app test drives the real on-device backend with a fake provider.
