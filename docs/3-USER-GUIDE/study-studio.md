# Study Studio (fork preview)

Open a notebook and select **Study Studio** beneath its header. Choose Flashcards or Quiz, 1–20 questions, and a difficulty. Select processed sources, optionally describe your learning focus, then select **Generate and save**. The configured default transformation model generates content in the current UI language; normal provider API charges apply.

Flashcards hide the answer until you reveal it. Mark each card **Got it** or **Missed it**. Quizzes lock your first answer and explain the correct choice. At the end, review everything or only missed items. Closing and reopening a deck starts a fresh session; scores are not saved.

Saved decks are stored in notebook Notes and reappear in Studio after reload. Delete a deck's note to remove it. The Notes viewer shows structured JSON; editing that JSON can make a deck unreadable. Studio skips malformed decks instead of failing the entire list.

Only sources loaded in the notebook's Sources column appear in this first version's picker. Load more there if needed. Unprocessed or empty sources are rejected. Select fewer sources if you exceed the input limit. An invalid model response can be retried manually; generation is never retried automatically.

Source references identify the source used by the model, but are not independently verified evidence. Review the original material when checking answers.

This preview includes flashcards and quizzes. Video, mind maps, slides, infographics, reports, data-table exports, and translated interface labels are still pending.
