"""Podcast orchestration with a Live audio adapter and ordinary TTS interoperability."""

import json
from pathlib import Path

from podcast_creator import create_podcast as conventional_create_podcast
from podcast_creator.episodes import load_episode_config
from podcast_creator.language import resolve_language_name
from podcast_creator.nodes import (
    combine_audio_node,
    generate_outline_node,
    generate_single_audio_clip,
    generate_transcript_node,
)
from podcast_creator.speakers import load_speaker_config

from open_notebook.ai.live_tts import LiveTextToSpeechModel
from open_notebook.ai.usage import record_podcast_audio


async def create_podcast(
    *, content, briefing, episode_name, output_dir, speaker_config, episode_profile
):
    profile = load_speaker_config(speaker_config)
    uses_live = profile.tts_provider == "openai" and profile.tts_model == "gpt-live-1"
    uses_live = uses_live or any(
        s.tts_model == "gpt-live-1"
        and (s.tts_provider or profile.tts_provider) == "openai"
        for s in profile.speakers
    )
    if not uses_live:
        return await conventional_create_podcast(
            content=content,
            briefing=briefing,
            episode_name=episode_name,
            output_dir=output_dir,
            speaker_config=speaker_config,
            episode_profile=episode_profile,
        )
    episode = load_episode_config(episode_profile)
    output = Path(output_dir)
    output.mkdir(parents=True, exist_ok=True)
    state = {
        "content": content,
        "briefing": briefing,
        "num_segments": episode.num_segments,
        "language": resolve_language_name(episode.language)
        if episode.language
        else None,
        "outline": None,
        "transcript": [],
        "audio_clips": [],
        "final_output_file_path": None,
        "output_dir": output,
        "episode_name": episode_name,
        "speaker_profile": profile,
        "json_mode_models": [],
    }
    config = {
        "configurable": {
            "outline_provider": episode.outline_provider,
            "outline_model": episode.outline_model,
            "outline_config": episode.outline_config,
            "transcript_provider": episode.transcript_provider,
            "transcript_model": episode.transcript_model,
            "transcript_config": episode.transcript_config,
            "audio_gap_ms": episode.audio_gap_ms,
        }
    }
    # Preserve the established source-grounded writing and validation nodes.
    state.update(await generate_outline_node(state, config))
    state.update(await generate_transcript_node(state, config))
    (output / "outline.json").write_text(state["outline"].model_dump_json())
    (output / "script.json").write_text(
        json.dumps([d.model_dump() for d in state["transcript"]], indent=2)
    )
    voices = profile.get_voice_mapping()
    # Sequential sessions bound spend and avoid connection spikes. No implicit retry.
    for index, dialogue in enumerate(state["transcript"]):
        speaker = profile.get_speaker_by_name(dialogue.speaker)
        provider = speaker.tts_provider or profile.tts_provider
        model = speaker.tts_model or profile.tts_model
        settings = (
            speaker.tts_config if speaker.tts_config is not None else profile.tts_config
        )
        info = {
            "dialogue": dialogue,
            "index": index,
            "output_dir": output,
            "tts_provider": provider,
            "tts_model": model,
            "tts_config": settings,
            "voices": voices,
        }
        if provider == "openai" and model == "gpt-live-1":
            path = output / "clips" / f"{index:04d}.mp3"
            response = await LiveTextToSpeechModel(
                model_name=model, config=settings
            ).agenerate_speech(
                dialogue.dialogue,
                voices[dialogue.speaker],
                path,
            )
            dialogue.dialogue = response.metadata["spoken_transcript"]
        else:
            path = await generate_single_audio_clip(info)
            await record_podcast_audio(path, model, episode_name + str(index))
        state["audio_clips"].append(path)
    (output / "transcript.json").write_text(
        json.dumps([d.model_dump() for d in state["transcript"]], indent=2)
    )
    state.update(await combine_audio_node(state, config))
    return {
        **state,
        "audio_clips_count": len(state["audio_clips"]),
        "audio_usage_recorded": True,
    }
