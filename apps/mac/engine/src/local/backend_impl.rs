//! `Backend` for `LocalBackend`: each call goes to the engine method of the same name.

use super::LocalBackend;
use crate::backend::{Backend, BackendKind, Cancel, EpisodeAudio, PodcastMode, Result, SendChat, SendChatResult, SendSourceChat};
use crate::types::*;

impl Backend for LocalBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Local
    }
    fn streams_chat(&self) -> bool {
        true
    }
    fn podcast_mode(&self) -> PodcastMode {
        PodcastMode::Local
    }

    fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        self.0.list_notebooks()
    }
    fn get_notebook(&self, id: &str) -> Result<Notebook> {
        self.0.get_notebook(id)
    }
    fn create_notebook(&self, name: &str, description: &str) -> Result<Notebook> {
        self.0.create_notebook(name, description)
    }
    fn update_notebook(&self, id: &str, update: NotebookUpdate) -> Result<Notebook> {
        self.0.update_notebook(id, update)
    }
    fn delete_notebook(&self, id: &str) -> Result<()> {
        self.0.delete_notebook(id)
    }

    fn list_sources(&self, notebook_id: &str) -> Result<Vec<SourceSummary>> {
        self.0.list_sources(notebook_id)
    }
    fn get_source(&self, id: &str) -> Result<SourceDetail> {
        self.0.get_source(id)
    }
    fn add_source(&self, notebook_id: &str, source: NewSource) -> Result<SourceSummary> {
        self.0.add_source(notebook_id, source)
    }
    fn list_all_sources(&self) -> Result<Vec<SourceSummary>> {
        self.0.list_all_sources()
    }
    fn link_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.0.link_source(source_id, notebook_id)
    }
    fn unlink_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.0.unlink_source(source_id, notebook_id)
    }
    fn rename_source(&self, id: &str, title: &str) -> Result<SourceDetail> {
        self.0.rename_source(id, title)
    }
    fn download_source(&self, id: &str) -> Result<Option<SourceFile>> {
        self.0.download_source(id)
    }
    fn retry_source(&self, id: &str) -> Result<()> {
        self.0.retry_source(id)
    }
    fn delete_source(&self, id: &str) -> Result<()> {
        self.0.delete_source(id)
    }

    fn list_notes(&self, notebook_id: &str) -> Result<Vec<Note>> {
        self.0.list_notes(notebook_id)
    }
    fn create_note(&self, notebook_id: &str, title: Option<&str>, content: &str, ai: bool) -> Result<Note> {
        self.0.create_note(notebook_id, title, content, ai)
    }
    fn update_note(&self, id: &str, title: Option<&str>, content: Option<&str>) -> Result<Note> {
        self.0.update_note(id, title, content)
    }
    fn delete_note(&self, id: &str) -> Result<()> {
        self.0.delete_note(id)
    }

    fn list_chat_sessions(&self, notebook_id: &str) -> Result<Vec<ChatSession>> {
        self.0.list_chat_sessions(notebook_id)
    }
    fn get_chat_session(&self, id: &str) -> Result<ChatSessionWithMessages> {
        self.0.get_chat_session(id)
    }
    fn send_chat_message(&self, args: SendChat<'_>) -> Result<SendChatResult> {
        self.0.send_chat_message(args)
    }
    fn delete_chat_session(&self, id: &str) -> Result<()> {
        self.0.delete_chat_session(id)
    }
    fn rename_chat_session(&self, id: &str, title: &str) -> Result<()> {
        self.0.rename_chat_session(id, title)
    }

    fn list_source_chat_sessions(&self, source_id: &str) -> Result<Vec<ChatSession>> {
        self.0.list_source_chat_sessions(source_id)
    }
    fn get_source_chat_session(&self, _source_id: &str, session_id: &str) -> Result<ChatSessionWithMessages> {
        self.0.get_chat_session(session_id)
    }
    fn send_source_chat_message(&self, args: SendSourceChat<'_>) -> Result<SendChatResult> {
        self.0.send_source_chat_message(args)
    }
    fn delete_source_chat_session(&self, _source_id: &str, session_id: &str) -> Result<()> {
        self.0.delete_chat_session(session_id)
    }

    fn search(&self, query: &str, notebook_id: Option<&str>) -> Result<Vec<SearchHit>> {
        self.0.search(query, notebook_id)
    }
    fn ask(&self, question: &str, notebook_id: Option<&str>, on_step: &mut (dyn FnMut(AskStep) + Send), cancel: &Cancel) -> Result<AskResult> {
        self.0.ask(question, notebook_id, on_step, cancel)
    }

    fn generate_study_deck(&self, notebook_id: &str, request: StudyRequest) -> Result<StudyDeck> {
        self.0.generate_study_deck(notebook_id, request)
    }
    fn list_study_decks(&self, notebook_id: &str) -> Result<Vec<StoredDeck>> {
        self.0.list_study_decks(notebook_id)
    }

    fn list_transformations(&self) -> Result<Vec<Transformation>> {
        self.0.list_transformations()
    }
    fn save_transformation(&self, t: Transformation) -> Result<Transformation> {
        self.0.save_transformation(t)
    }
    fn delete_transformation(&self, id: &str) -> Result<()> {
        self.0.delete_transformation(id)
    }
    fn run_transformation(&self, transformation_id: &str, text: &str, cancel: &Cancel) -> Result<String> {
        self.0.run_transformation(transformation_id, text, cancel)
    }
    fn transformation_instructions(&self) -> Result<String> {
        self.0.transformation_instructions()
    }
    fn set_transformation_instructions(&self, text: &str) -> Result<()> {
        self.0.set_transformation_instructions(text)
    }

    fn list_insights(&self, source_id: &str) -> Result<Vec<Insight>> {
        self.0.list_insights(source_id)
    }
    fn create_insight(&self, source_id: &str, transformation_id: &str) -> Result<()> {
        self.0.create_insight(source_id, transformation_id)
    }
    fn delete_insight(&self, id: &str) -> Result<()> {
        self.0.delete_insight(id)
    }
    fn save_insight_as_note(&self, insight_id: &str, notebook_id: &str) -> Result<Note> {
        self.0.save_insight_as_note(insight_id, notebook_id)
    }

    fn usage(&self, days: u32) -> Result<UsageReport> {
        self.0.usage(days)
    }
    fn rebuild_index(&self) -> Result<u32> {
        self.0.rebuild_index()
    }
    fn list_models(&self) -> Result<Vec<ServerModel>> {
        Ok(Vec::new())
    }

    fn list_episodes(&self, notebook_id: &str) -> Result<Vec<Episode>> {
        self.0.list_episodes(notebook_id)
    }
    fn get_episode(&self, id: &str) -> Result<Episode> {
        self.0.get_episode(id)
    }
    fn create_episode(&self, input: CreateEpisode) -> Result<Episode> {
        self.0.create_episode(input)
    }
    fn delete_episode(&self, id: &str) -> Result<()> {
        self.0.delete_episode(id)
    }
    fn episode_audio(&self, id: &str) -> Result<EpisodeAudio> {
        self.0.episode_audio(id)
    }
    fn episode_prompt(&self, id: &str) -> Result<Option<EpisodePrompt>> {
        self.0.episode_prompt(id)
    }
    fn check_episode(&self, id: &str) -> Result<()> {
        self.0.check_episode(id)
    }
    fn retry_episode(&self, id: &str) -> Result<()> {
        self.0.retry_episode(id)
    }
    fn podcast_profiles(&self) -> Result<PodcastProfiles> {
        self.0.podcast_profiles()
    }
    fn save_episode_profile(&self, p: EpisodeProfile) -> Result<EpisodeProfile> {
        self.0.save_episode_profile(p)
    }
    fn delete_episode_profile(&self, id: &str) -> Result<()> {
        self.0.delete_episode_profile(id)
    }
    fn save_speaker_profile(&self, p: SpeakerProfile) -> Result<SpeakerProfile> {
        self.0.save_speaker_profile(p)
    }
    fn delete_speaker_profile(&self, id: &str) -> Result<()> {
        self.0.delete_speaker_profile(id)
    }
}
