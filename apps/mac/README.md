# Open Notebook for macOS (Rune)

The on-device Mac app, drawn with [Rune](https://github.com/anomalyco/rune). It runs notebooks on this Mac with your own API keys, or connects to an Open Notebook server. iOS stays on the Tauri app in `apps/desktop` ([ADR-019](../../docs/7-DEVELOPMENT/decisions/ADR-019-rune-mac-app.md)).

- `engine/`: `notebook-engine`, the Rust port of `packages/engine` (on-device backend on SQLite, AI providers, podcasts) and `packages/api-client` (server backend). Same schema, prompts and data folder as the TypeScript engine.
- `app/`: the Rune UI. `state.rs` holds the app state and runs backend calls on worker threads; `ui/` draws it.

## Run

On a Mac with Rust installed:

```sh
cargo run -p open-notebook-mac            # opens the window
scripts/bundle.sh                         # builds target/bundle/Open Notebook.app
```

Data lives in `~/Library/Application Support/com.donovanmontoya.opennotebook`, the Tauri app's folder, so both apps read the same notebooks, `keys.json` and `originals/`. Set `OPEN_NOTEBOOK_DATA_DIR` to use another folder.

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
