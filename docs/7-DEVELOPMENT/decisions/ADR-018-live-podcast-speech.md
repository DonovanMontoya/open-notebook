# ADR-018: GPT Live as a saved-podcast speech renderer

Status: Accepted, 2026-11-01

GPT Live is selectable as text-to-speech through an application adapter. It uses the Live WebSocket API, not the conventional audio/speech endpoint. Existing Live model records are reclassified without changing their IDs.

The podcast writer retains its existing outline and transcript nodes. When a profile or individual speaker selects gpt-live-1, application-owned orchestration renders dialogue through bounded Live sessions; other voices retain ordinary TTS. No library monkey patches or process-wide provider overrides are used. The default model manager also provisions the adapter.

Each request carries at most 220 script tokens plus fixed narration instructions. The server feeds silence while receiving 24kHz PCM, waits three seconds after the final audible output and transcript activity, closes the session, and requires finalization. A 150-second hard deadline bounds each session. Captured speech is checked against the script for substantial coverage and final words. No silent fallback or automatic paid retry occurs. Live may paraphrase; stored episode transcripts use returned spoken text, with the original script retained as script.json. Failed narration never publishes a partial episode.

Speech is encoded to MP3 and enters the existing combination, signed range streaming, persistent player, and download paths. Live sessions run sequentially and their provider-reported seconds are recorded individually, including failed finalized attempts; estimated elapsed time is labeled when final usage is unavailable. Mixed conventional clips receive separate duration estimates, avoiding duplicate episode-level charges.

Credentials remain server-side and the adapter requires the direct OpenAI endpoint. Only generated narration is sent to Live. Model setup changes default and speaker-profile choices on this installation at the user's request; historical episodes are unchanged.

References: https://developers.openai.com/api/docs/guides/voice-websockets and https://developers.openai.com/api/docs/guides/live-conversations.
