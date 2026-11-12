from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import pytest
from podcast_creator import configure
from podcast_creator.core import Dialogue

from open_notebook.podcasts.live_generation import create_podcast


@pytest.mark.asyncio
async def test_mixed_speakers_route_live_and_native_without_double_billing(tmp_path):
    configure(
        "speakers_config",
        {
            "profiles": {
                "mixed": {
                    "tts_provider": "openai",
                    "tts_model": "gpt-live-1",
                    "tts_config": {"api_key": "test"},
                    "speakers": [
                        {
                            "name": "Live",
                            "voice_id": "marin",
                            "backstory": "Host",
                            "personality": "Clear",
                        },
                        {
                            "name": "Native",
                            "voice_id": "alloy",
                            "backstory": "Host",
                            "personality": "Clear",
                            "tts_provider": "openai",
                            "tts_model": "gpt-4o-mini-tts",
                        },
                    ],
                }
            }
        },
    )
    configure(
        "episode_config",
        {"profiles": {"short": {"speaker_config": "mixed", "num_segments": 1}}},
    )
    outline = SimpleNamespace(model_dump_json=lambda: "{}")
    dialogue = [
        Dialogue(speaker="Live", dialogue="Hello notebook."),
        Dialogue(speaker="Native", dialogue="Hello listener."),
    ]
    module = "open_notebook.podcasts.live_generation"
    with (
        patch(
            module + ".generate_outline_node",
            new_callable=AsyncMock,
            return_value={"outline": outline},
        ),
        patch(
            module + ".generate_transcript_node",
            new_callable=AsyncMock,
            return_value={"transcript": dialogue},
        ),
        patch(
            module + ".LiveTextToSpeechModel.agenerate_speech",
            new_callable=AsyncMock,
            return_value=SimpleNamespace(
                metadata={"spoken_transcript": "Hello, notebook."}
            ),
        ) as live,
        patch(
            module + ".generate_single_audio_clip",
            new_callable=AsyncMock,
            return_value=tmp_path / "clips/0001.mp3",
        ) as native,
        patch(
            module + ".combine_audio_node",
            new_callable=AsyncMock,
            return_value={"final_output_file_path": tmp_path / "episode.mp3"},
        ),
        patch(module + ".record_podcast_audio", new_callable=AsyncMock) as usage,
    ):
        result = await create_podcast(
            content="Synthetic",
            briefing="Brief",
            episode_name="test",
            output_dir=str(tmp_path),
            speaker_config="mixed",
            episode_profile="short",
        )
    assert live.await_count == 1
    assert native.await_count == 1
    assert usage.await_count == 1
    assert usage.await_args is not None
    assert usage.await_args.args[1] == "gpt-4o-mini-tts"
    assert result["audio_usage_recorded"]
    assert result["transcript"][0].dialogue == "Hello, notebook."
    assert "Hello notebook." in (tmp_path / "script.json").read_text()
    assert "Hello, notebook." in (tmp_path / "transcript.json").read_text()


@pytest.mark.asyncio
async def test_native_profiles_keep_established_generator(tmp_path):
    configure(
        "speakers_config",
        {
            "profiles": {
                "native": {
                    "tts_provider": "openai",
                    "tts_model": "gpt-4o-mini-tts",
                    "speakers": [
                        {
                            "name": "Host",
                            "voice_id": "alloy",
                            "backstory": "Host",
                            "personality": "Clear",
                        }
                    ],
                }
            }
        },
    )
    with patch(
        "open_notebook.podcasts.live_generation.conventional_create_podcast",
        new_callable=AsyncMock,
        return_value={"final_output_file_path": "ok"},
    ) as conventional:
        result = await create_podcast(
            content="Synthetic",
            briefing="Brief",
            episode_name="test",
            output_dir=str(tmp_path),
            speaker_config="native",
            episode_profile="short",
        )
    conventional.assert_awaited_once()
    assert result["final_output_file_path"] == "ok"
