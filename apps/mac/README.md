# Open Notebook for macOS and iPhone (Rune)

The on-device Mac app, drawn with [Rune](https://github.com/anomalyco/rune). It runs notebooks on this Mac with your own API keys, or connects to an Open Notebook server. The same app runs on iPhone and the iPhone Duo ([ADR-019](../../docs/7-DEVELOPMENT/decisions/ADR-019-rune-mac-app.md), [ADR-020](../../docs/7-DEVELOPMENT/decisions/ADR-020-rune-iphone-app.md)).

- `engine/`: `notebook-engine`, the Rust port of `packages/engine` (on-device backend on SQLite, AI providers, podcasts) and `packages/api-client` (server backend). Same schema, prompts and data folder as the TypeScript engine.
- `app/`: the Rune UI. `state.rs` holds the app state and runs backend calls on worker threads; `ui/` draws it, with `ui/mobile.rs` for the phone and the Duo. `ios.rs` is the C interface the iPhone shell calls.
- `ios/`: the iPhone shell, a small SwiftUI app that hosts the Rust library in a Metal layer and passes it touches, typed text, the fold and the safe area.

## Run

On a Mac with Rust installed:

```sh
cargo run -p open-notebook-mac            # opens the window
scripts/bundle.sh                         # builds target/bundle/Open Notebook.app
```

Data lives in `~/Library/Application Support/com.donovanmontoya.opennotebook`, the Tauri app's folder, so both apps read the same notebooks, `keys.json` and `originals/`. Set `OPEN_NOTEBOOK_DATA_DIR` to use another folder.

### iPhone

On the Mac (not over SSH; signing needs the keychain), with rustup's `aarch64-apple-ios` target and an App Store Connect API key in `~/.appstoreconnect/` (or `ASC_KEY_JSON`):

```sh
scripts/iphone.sh                         # builds, signs, installs and launches on the connected iPhone
```

For the simulator, build `--target aarch64-apple-ios-sim` and run `ios/OpenNotebook.xcodeproj`. The app installs as "Notebook (Rune)" and keeps its data in its sandbox.

### PDF reading on Mac

Library embeds original PDF pages in its main content area, using PDFKit for page thumbnails, selection, search and zoom. Swift is required when building the Mac app; Cargo compiles and embeds the native renderer automatically. Distributed builds need no compiler or separate reader app.

The Discussion panel asks questions about the open material. Selecting text adds it to the question context; **Highlight & ask** also marks it on the page. **Figures** detects numbered figure captions and pairs each one with the original page image and passages mentioning its number. Related-text links select the passage in the PDF; page links in answers navigate inside the reader.

Drag the divider beside Discussion to resize it; double-click resets the width. The width is remembered across materials and launches. Choose **Visualize** or **Animate** on a selected figure, or ask for an interactive explanation in chat. Completed HTML answers appear as an **Open interactive figure** action and open in the Library's Visualization tab. Original paper returns to the PDF; Restart resets the visualization. The model is instructed to provide play, pause, step and reset controls for animations and to identify schematic interpretations. Generated code is saved with the conversation, so it can be opened again later.

HTML runs in a sandboxed frame inside a private WebKit view. It has no host-app access or external network resources. Its only supported host message is a validated PDF page link. Hiding the view unloads the document and stops its scripts. Incomplete and oversized HTML answers display a recovery message rather than executing.

Choose a chat model in Settings before asking. Figure images require on-device mode and a model that accepts images; server mode supports text questions. The full figure page is sent so charts are not cut off by a guessed crop. Caption detection needs selectable text and does not detect unlabeled illustrations or scanned pages. Highlights and the pinned selection last while the material is open; conversation text is saved through source chat, which requires notebook membership. Page images are attached to questions while pinned and are not stored in conversation history.

### Settings

Settings has four sections: Connection, API keys, Models and Preferences. A saved key is shown separately from a successful access check. Key checks list models (or ElevenLabs voices) without generating content. Verification lasts for the open Settings session; returning to Settings checks saved model-provider keys again.

**Recommended models** checks providers with saved keys and selects supported models from their returned catalogs. It favors a balanced chat model, a smaller embedding model and a supported transcription model. Preview and experimental models are excluded. Supported selections are saved immediately; roles without a recommended option keep their current selection. A checked local model endpoint can also be used without a key. Existing materials need a search-index rebuild after an embedding-model change.

**Save and connect** verifies server access before saving the address and switching notebooks. A failed connection keeps the current backend active. Editing the address cancels the pending result, so an old check cannot activate a different server. Server model and provider administration remains on the Server page.

## Check

```sh
cargo test                                # engine and app tests
cargo clippy --all-targets
# Render a screen headless, with sample data and no backend:
cargo run -p open-notebook-mac -- --snap notebook.png --scene notebook --size 1280x800 [--theme dark]
cargo run -p open-notebook-mac -- --list-scenes
```

`OPEN_NOTEBOOK_TEST_DATA=<copy of a data folder> cargo test -p open-notebook-mac -- --ignored` opens a copy of real data. Use a copy: opening marks unfinished work as failed.

The engine builds and tests on Linux. The app needs a GPU, so build it and take snapshots on a Mac.

## What differs from the Tauri app

- **Volume:** there's no volume control. Rune's `Player` keeps its AVPlayer private and exposes only mute.
- **Live talk:** it runs on device only, over OpenAI's websocket. The server's `/live/session` accepts only a WebRTC offer, so server mode says to switch to on-device mode.
- **Dropped links:** dropped links and text aren't added; dropped files are. On macOS, Rune hands over file paths only.
- **Multi-select:** "Choose file" picks one file at a time, because Rune's open dialog has no multi-select. Dropping several files works.
- **iCloud Keychain sync:** it needs a signed build with the keychain entitlement. The ad-hoc build from `scripts/bundle.sh` says so and keeps keys in `keys.json`.
- **Small choices:** remembered choices live in `prefs.json` in the data folder, not the web view. These are rate, resume positions, chat context, the model per conversation, effort and voice.
