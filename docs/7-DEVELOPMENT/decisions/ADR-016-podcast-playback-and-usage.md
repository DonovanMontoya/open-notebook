# ADR-016: Persistent playback and usage visibility

Keep one native audio element in the authenticated dashboard layout. Episode cards control it through context; navigation preserves it and logout unmounts it. AppShell reserves player space. Reloading or closing the tab ends playback.

Native HTTP ranges and attachment downloads replace whole-file JavaScript Blobs. An authenticated audio-access POST issues a 24-hour HMAC ticket scoped to one contained audio path, accepted only for GET/HEAD. Password changes revoke tickets. Responses are private, non-cacheable. Tickets are temporary media credentials, not share links.

Persist usage metadata in usage_event (migration 26), never prompts or credentials. LangChain callbacks record provider token metadata for notebook calls and podcast writing. Embeddings use tokenizer estimates; completed podcast audio uses measured duration and an approximate minute rate. Failures in accounting cannot fail generation. Standard USD list prices checked 2026-10-31 come from https://developers.openai.com/api/docs/pricing and https://platform.claude.com/docs/en/models/sonnet-5-5/overview. Unknown models are unpriced. The UI explicitly states coverage and exclusions: this is not an invoice, and no historical activity is fabricated.
