# ADR-015: Voice-only GPT Live sessions

The server exchanges a browser WebRTC offer for an OpenAI Live answer using the default OpenAI chat model's linked credential. Only the session ID and SDP answer reach the browser; the project key stays server-side. GPT Live handles voice, with Responses delegation to the configured chat model. This does not alter podcast TTS or file transcription settings.

No notebook source content is read or transmitted by this integration. Source-aware delegation remains disabled pending explicit user authorization. The microphone is requested only after Start; the UI explains that audio goes to OpenAI and API billing applies. Session closure uses session.close/session.closed with a bounded timeout. Navigation and failed connections stop microphone tracks and close the connection. Sessions are capped client-side at 30 minutes. This is a personal tailnet deployment using the app's existing authentication boundary, not a multi-tenant service.

The provider protocol follows https://developers.openai.com/api/docs/guides/voice-webrtc and https://developers.openai.com/api/docs/guides/live-delegation. GPT Live Transcribe is a distinct streaming-transcription workflow and is not used for this speech-to-speech connection.
