"""Local usage ledger. Stores quantities only, never prompts or credentials."""

import asyncio
from contextlib import contextmanager
from contextvars import ContextVar
from datetime import datetime, timezone
from uuid import uuid4

from langchain_core.callbacks import AsyncCallbackHandler
from langchain_core.tracers.context import register_configure_hook
from loguru import logger

from open_notebook.database.repository import repo_upsert

# Standard USD list prices, verified 2026-10-31. Estimates exclude tools,
# regional/service-tier premiums and long-context adjustments.
# https://developers.openai.com/api/docs/pricing
# https://platform.claude.com/docs/en/models/sonnet-5-5/overview
RATES = {
    "gpt-6.1-sol": (2.0, 10.0, 0.10),
    "gpt-6.1-terra": (1.0, 5.0, 0.05),
    "gpt-6-luna": (0.10, 0.50, 0.01),
    "claude-sonnet-5-5": (2.0, 10.0, 0.20),
    "text-embedding-3-large": (0.13, 0.0, 0.13),
    "text-embedding-3-small": (0.02, 0.0, 0.02),
}


def estimate_cost(
    model: str,
    input_tokens: int,
    output_tokens: int,
    cached: int = 0,
    written: int = 0,
    seconds: float = 0,
):
    if model == "gpt-live-1":
        return seconds * 0.05 / 60
    if model.startswith("gpt-4o-mini-tts"):
        # Published approximate $/minute; includes average text/audio token mix.
        return seconds * 0.015 / 60
    rates = RATES.get(model)
    if rates is None:
        return None
    base, output, cache = rates
    return (
        max(0, input_tokens - cached - written) * base
        + output_tokens * output
        + cached * cache
        + written * base * 1.25
    ) / 1_000_000


async def record_usage(
    *,
    model: str,
    feature: str,
    input_tokens: int = 0,
    output_tokens: int = 0,
    cached: int = 0,
    written: int = 0,
    seconds: float = 0,
    basis: str = "provider_tokens",
    event_id: str | None = None,
):
    if not isinstance(model, str):
        return
    try:
        cost = (
            None
            if basis == "usage_unavailable"
            else estimate_cost(
                model, input_tokens, output_tokens, cached, written, seconds
            )
        )
        await asyncio.wait_for(
            repo_upsert(
                "usage_event",
                f"usage_event:{event_id or uuid4().hex}",
                {
                    "model": model,
                    "feature": feature,
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "cached_tokens": cached,
                    "cache_write_tokens": written,
                    "audio_seconds": seconds,
                    "estimated_usd": cost,
                    "basis": basis,
                    "pricing_date": "2026-10-31",
                    "created": datetime.now(timezone.utc),
                },
            ),
            timeout=3,
        )
    except Exception:
        logger.warning("Usage record unavailable; model operation continues")


class UsageCallback(AsyncCallbackHandler):
    def __init__(self, feature: str, model: str = "unknown"):
        self.feature = feature
        self.model = model

    async def on_llm_end(self, response, *, run_id, **kwargs):
        try:
            message = response.generations[0][0].message
            usage = message.usage_metadata
            model = (
                message.response_metadata.get("model_name")
                or message.response_metadata.get("model")
                or self.model
            )
            if not usage:
                await record_usage(
                    model=model,
                    feature=self.feature,
                    basis="usage_unavailable",
                    event_id=run_id.hex,
                )
                return
            details = usage.get("input_token_details") or {}
            await record_usage(
                model=model,
                feature=self.feature,
                input_tokens=usage.get("input_tokens", 0),
                output_tokens=usage.get("output_tokens", 0),
                cached=details.get("cache_read", 0),
                written=details.get("cache_creation", 0),
                event_id=run_id.hex,
            )
        except Exception:
            logger.warning("Model did not expose usable usage metadata")


_podcast_callback: ContextVar[UsageCallback | None] = ContextVar(
    "podcast_usage", default=None
)
register_configure_hook(_podcast_callback, inheritable=True)


@contextmanager
def track_podcast_usage():
    token = _podcast_callback.set(UsageCallback("podcast_writing"))
    try:
        yield
    finally:
        _podcast_callback.reset(token)


async def record_podcast_audio(path, model: str, episode_id: str):
    """Duration-based estimate for a completed episode; not a provider receipt."""
    try:
        process = await asyncio.create_subprocess_exec(
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            str(path),
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
        )
        try:
            stdout, _ = await asyncio.wait_for(process.communicate(), timeout=15)
        except TimeoutError:
            process.kill()
            await process.wait()
            return
        if process.returncode == 0:
            await record_usage(
                model=model,
                feature="podcast_audio",
                seconds=float(stdout),
                basis="audio_duration_estimate",
                event_id=episode_id.split(":")[-1] + "audio",
            )
    except Exception:
        logger.warning("Could not estimate podcast audio usage")
