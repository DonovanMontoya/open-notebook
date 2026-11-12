# ADR-020: The Rune app also runs on iPhone, inside a small Swift shell

- **Status**: Accepted
- **Date**: 2026-11
- **Related**: [ADR-019](ADR-019-rune-mac-app.md) (supersedes its point 3)

## Context

ADR-019 kept iOS on Tauri because Rune had no iOS target. Rune can now draw into a layer the host owns (`rune::embed::LayerHost::from_core_animation_layer`) and takes touches, keys and committed text through its runtime. The window, the display link and the keyboard are left to the host, which is what an iOS app needs.

The iPhone Duo matters most: folded it is a phone (about 466 x 678 pt), open it is close to a small tablet (about 951 x 669 pt), and partly folded it reports the fold through `UIView.reservedRegions(kind: .division)` (iOS 27.1).

## Decision

1. The `apps/mac/app` crate also builds as a static library (`open_notebook`) with a C interface in `src/ios.rs`. The same UI and engine code run on macOS and iOS.
2. `apps/mac/ios` is a minimal SwiftUI app. It owns a `CAMetalLayer` view, a 120 Hz `CADisplayLink`, touches, `UIKeyInput` and the safe area, and passes them to Rust. It sends the fold and the bottom inset so the layout can avoid them.
3. Layout picks a size class from the window width: Phone (< 560 pt) gets one column with a back bar and bottom tabs; Pages (< 900 pt) and an open Duo get two facing pages that meet at the fold when there is one; Desk is the Mac layout.
4. The iPhone build installs as its own app (`com.donovanmontoya.opennotebook.rune`, "Notebook (Rune)") next to the Tauri one, until it replaces it. Data stays in the app's sandbox.

## Alternatives considered

- **Keep iOS on Tauri.** One more release of two UIs and two engines, with the webview problems ADR-019 lists.
- **Wait for Rune's own iOS shell.** The embed path already works, and the shell is about 200 lines of Swift that can be dropped if Rune ships one.

## Consequences

- The Tauri app (`apps/desktop`) and the TypeScript engine are no longer needed on any platform once the iPhone build covers its features.
- Signing runs on the Mac with an App Store Connect API key (`scripts/iphone.sh`); it can't run over SSH because the keychain is locked there.
- Rune's text input on iOS is committed text only: no marked text, so no autocorrect suggestions or dictation preview yet.
- Phone and Duo screens are checked headlessly at their real sizes and in the iPhone Duo simulator.
