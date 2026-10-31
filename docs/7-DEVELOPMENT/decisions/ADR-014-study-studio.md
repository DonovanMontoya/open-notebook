# ADR-014: Versioned study decks in notebook notes

Status: Accepted for the fork's first Studio increment.

Study Studio generates flashcards and multiple-choice quizzes from explicitly selected notebook sources using the existing transformation-model provisioner. The API validates the generated schema, count, quiz options, and source membership before saving. Source IDs establish provenance but do not prove that a claim is entailed by source text.

Decks are versioned JSON in AI notes with a reserved `<!-- open-notebook-study:v1 -->` prefix. This reuses notebook relationships, backup, and note deletion without a migration. Editing the structured note can invalidate a deck; invalid notes are omitted from Studio. The ordinary Notes viewer currently displays the serialized deck. A future artifact domain should replace this storage when adding larger media outputs.

Generation is synchronous, bounded to 20 items and 300,000 input characters. The frontend never retries a generation automatically. Generation errors do not save partial output. Note save and notebook linkage retain the existing note API's two-operation behavior. Study session scores are ephemeral; saved decks persist, but answers reset when a session is reopened.

Initial scope: flashcards and quizzes. Not yet implemented: narrated videos, mind maps, slide decks, infographics, report/data-table exports, persisted review history, and translated Studio UI labels. The source picker lists sources already loaded in the notebook; load more sources in the Sources column to select additional ones.
