from open_notebook.ai.model_discovery import classify_model_type
from open_notebook.ai.openai_compat import GPT6LanguageModel


def test_gpt6_langchain_uses_responses_without_sampling_controls():
    model = GPT6LanguageModel(
        model_name="gpt-6.1-sol", config={"api_key": "test-only", "max_tokens": 123}
    )
    chat = model.to_langchain()
    assert chat.use_responses_api is True
    assert chat.temperature is None
    assert chat.top_p is None
    assert chat.max_tokens == 123


def test_openai_audio_and_embedding_classification():
    assert classify_model_type("gpt-transcribe", "openai") == "speech_to_text"
    assert classify_model_type("gpt-4o-transcribe", "openai") == "speech_to_text"
    assert (
        classify_model_type("gpt-4o-mini-tts-2025-12-15", "openai") == "text_to_speech"
    )
    assert classify_model_type("text-embedding-3-large", "openai") == "embedding"
