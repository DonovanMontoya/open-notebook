# ADR-022: Settings setup and verified connections

- **Status**: Accepted
- **Date**: 2026-11-15

## Context

One long Settings form mixed provider setup, model choices, appearance and server connection. Saved credentials looked similar to untested ones. Switching the mode before saving also prevented reconnect logic from detecting the change.

## Decision

Connection, API keys, Models and Preferences have separate tabs. The active backend and draft connection mode are separate state. Server access must pass authentication-status and notebook-list requests before the address and mode are committed and the backend is replaced. A failed connection leaves the previous backend active. Pasted passwords are stored by the explicit connection action and remain hidden.

Checks and recommendations run independently of an open backend. Each operation has a request identity; results from replaced settings forms, edited addresses or removed keys are ignored. Field accessors only access editing state; change callbacks invalidate checks after actual edits. Rendering a form must not cancel its requests.

Key status distinguishes absence, saved but unchecked, checking, verified access and failure. Model-list access is not a guarantee of billing eligibility or permission to invoke every model. Save results appear beside the action and in a toast. Key fields stay empty after saving.

Recommendations use a curated preference order, intersected with catalogs fetched using available saved keys. They favor Sonnet for chat and figures, then a general OpenAI model or Gemini Flash; OpenAI small embeddings and transcription are preferred when available. Compatible endpoints use recognized catalog entries or a single exposed model. Preview, experimental and unrelated model types are excluded. Recommendations save supported roles and retain roles without a suitable option. Index rebuilding remains explicit because it processes existing materials.

The curated families follow the [Claude model catalog](https://platform.claude.com/docs/en/models/overview), [OpenAI model catalog](https://developers.openai.com/api/docs/models) and [Gemini model catalog](https://ai.google.dev/gemini-api/docs/models), checked on 2026-11-15. They are application defaults, not claims that one model is best for every task.

## Consequences

The user can see what is saved, what was checked and what changed without leaving the relevant form. Provider checks do not generate answers. Network failures do not silently assign guessed model names or change the active notebook connection. Curation needs maintenance as providers retire model families; manual choices remain available. Server-managed models and keys use the existing Server administration page.
