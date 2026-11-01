import base64
import json
from unittest.mock import AsyncMock, patch

import pytest

from open_notebook.ai.live_tts import (
    LiveTextToSpeechModel,
    _render_part,
    narration_matches,
    split_narration,
)
from open_notebook.exceptions import ConfigurationError, ExternalServiceError


def test_narration_chunks_are_bounded_and_preserve_words():
    import tiktoken

    text = "A useful research finding. " * 300
    parts = split_narration(text)
    encoding = tiktoken.get_encoding("cl100k_base")
    assert len(parts) > 1
    assert all(len(encoding.encode(part)) <= 220 for part in parts)
    assert " ".join(parts) == text.strip()


@pytest.mark.parametrize(
    "spoken", ["", "Let me explain something else.", "A useful research"]
)
def test_narration_rejects_empty_unrelated_and_truncated(spoken):
    assert not narration_matches(
        "A useful research finding connects two ideas.", spoken
    )


def test_narration_accepts_punctuation_changes():
    assert narration_matches("Welcome to Open Notebook!", "Welcome, to Open Notebook.")


class FakeSocket:
    def __init__(self, events):
        self.events = iter(events)
        self.sent = []

    async def send(self, value):
        self.sent.append(json.loads(value))

    async def recv(self):
        return json.dumps(next(self.events))


class SocketContext:
    def __init__(self, socket):
        self.socket = socket

    async def __aenter__(self):
        return self.socket

    async def __aexit__(self, *args):
        return False


@pytest.mark.asyncio
async def test_stream_collects_pcm_and_records_provider_seconds():
    text = "Welcome to Open Notebook."
    pcm = b"\xff\x07" * 24000
    socket = FakeSocket(
        [
            {"type": "session.started", "session": {"id": "live_test"}},
            {
                "type": "session.output_audio.delta",
                "delta": base64.b64encode(pcm).decode(),
            },
            {"type": "session.output_transcript.delta", "delta": text},
            {"type": "session.closed", "usage": {"seconds": 8}},
        ]
    )
    with (
        patch("open_notebook.ai.live_tts.connect", return_value=SocketContext(socket)),
        patch(
            "open_notebook.ai.live_tts.record_usage", new_callable=AsyncMock
        ) as usage,
    ):
        result, transcript = await _render_part(text, "marin", "secret")
    assert result == pcm
    assert transcript == text
    assert socket.sent[0]["session"]["model"] == "gpt-live-1"
    assert socket.sent[1]["content"].endswith(text)
    assert usage.await_args is not None
    assert usage.await_args.kwargs["seconds"] == 8
    assert usage.await_args.kwargs["basis"] == "provider_session_seconds"


@pytest.mark.asyncio
async def test_incomplete_speech_fails_even_after_session_closed():
    socket = FakeSocket(
        [
            {"type": "session.started", "session": {"id": "live_test"}},
            {
                "type": "session.output_audio.delta",
                "delta": base64.b64encode(b"\xff\x07" * 100).decode(),
            },
            {"type": "session.output_transcript.delta", "delta": "Welcome"},
            {"type": "session.closed", "usage": {"seconds": 5}},
        ]
    )
    with (
        patch("open_notebook.ai.live_tts.connect", return_value=SocketContext(socket)),
        patch("open_notebook.ai.live_tts.record_usage", new_callable=AsyncMock),
    ):
        with pytest.raises(ExternalServiceError, match="complete script"):
            await _render_part("Welcome to Open Notebook.", "marin", "secret")


@pytest.mark.asyncio
async def test_invalid_voice_or_proxy_fails_before_network(tmp_path):
    with patch("open_notebook.ai.live_tts.connect") as connection:
        with pytest.raises(ConfigurationError, match="voice"):
            await LiveTextToSpeechModel(api_key="secret").agenerate_speech(
                "Hello", "nova", tmp_path / "clip.mp3"
            )
        with pytest.raises(ConfigurationError, match="direct OpenAI"):
            await LiveTextToSpeechModel(
                api_key="secret", base_url="https://proxy.example"
            ).agenerate_speech("Hello", "marin")
        connection.assert_not_called()
    assert not list(tmp_path.iterdir())


@pytest.mark.asyncio
async def test_model_manager_uses_live_adapter():
    from open_notebook.ai.models import Model, ModelManager

    model = Model(
        id="model:test", name="gpt-live-1", provider="openai", type="text_to_speech"
    )
    with (
        patch.object(Model, "get", new_callable=AsyncMock, return_value=model),
        patch(
            "open_notebook.ai.key_provider.provision_provider_keys",
            new_callable=AsyncMock,
        ),
    ):
        assert isinstance(
            await ModelManager().get_model("model:test"), LiveTextToSpeechModel
        )


def test_discovery_classifies_live_as_speech():
    from open_notebook.ai.model_discovery import classify_model_type

    assert classify_model_type("gpt-live-1", "openai") == "text_to_speech"
