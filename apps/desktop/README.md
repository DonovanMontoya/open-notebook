# Open Notebook app (macOS, then iOS)

A Tauri 2 + React app for Open Notebook with two modes:

- **This device.** Notebooks are stored in SQLite on the device. Sources are read and
  indexed locally, and questions go straight to the provider you choose (Anthropic,
  OpenAI, Gemini, or any OpenAI-compatible endpoint such as OpenRouter, LM Studio or
  Ollama) with your own API key. Keys are kept in the system keychain.
- **Open Notebook server.** The same interface on top of a running Open Notebook API,
  for local models and the full server feature set.

The UI talks to one `Backend` interface (`packages/engine/src/backend.ts`), implemented
by `LocalBackend` (on device) and `RemoteBackend` (server, via `packages/api-client`).

## Develop

From the repository root:

```sh
npm install
npm run dev -w @open-notebook/desktop          # UI in a browser at http://localhost:1420
npm run tauri -w @open-notebook/desktop -- dev # the real app window (macOS)
```

In a plain browser the app falls back to sql.js and localStorage so the interface can be
worked on without Tauri. Provider calls from the browser are subject to CORS; the app
itself uses the Tauri HTTP plugin and has no such limit.

## Build

```sh
cd apps/desktop
npx tauri build --bundles app   # macOS: src-tauri/target/release/bundle/macos/Open Notebook.app
```

## Test

```sh
npm test -w @open-notebook/engine      # storage, ingestion, retrieval, chat, providers, citations
npm test -w @open-notebook/api-client
npm run typecheck -w @open-notebook/desktop
```

## iCloud Keychain

Settings has a "Sync with iCloud Keychain" switch. Keys always go to the keychain; with
the switch on they go to iCloud Keychain instead, so they reach your other Macs and the
iOS app. That needs a signed build:

1. Register the App ID `com.donovanmontoya.opennotebook` on team WS7DWR8G56 with the
   Keychain Sharing capability.
2. Create a macOS development (or Developer ID) provisioning profile for it and save it
   as `src-tauri/embedded.provisionprofile`.
3. In `tauri.conf.json`, set `bundle.macOS.entitlements` to `Entitlements.icloud.plist`,
   `bundle.macOS.files` to `{ "embedded.provisionprofile": "embedded.provisionprofile" }`,
   and `bundle.macOS.signingIdentity` to the Apple Development certificate.

Unsigned builds report that sync is unavailable and keep keys in this Mac's keychain.

## Podcasts

The Listen tab makes episodes. On this device the chat model writes the script and a
voice provider reads it: OpenAI speech, OpenAI Live (`gpt-live-1`, recorded in real time
over its WebSocket API), Gemini (both hosts in one take) or ElevenLabs. Audio is saved
under the app data folder and converted to AAC with `afconvert` on macOS. Connected to a
server, episodes are made by the server's podcast profiles instead.
