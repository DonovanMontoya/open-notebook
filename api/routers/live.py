"""Voice-only GPT Live session negotiation; no notebook content is exported."""

from typing import Literal
from urllib.parse import urlsplit

import httpx
from fastapi import APIRouter, Request
from pydantic import BaseModel, Field

from open_notebook.ai.models import DefaultModels, Model
from open_notebook.exceptions import (
    ConfigurationError,
    ExternalServiceError,
    InvalidInputError,
)

router = APIRouter()


class LiveOffer(BaseModel):
    sdp: str = Field(min_length=20, max_length=65536)
    voice: Literal["marin", "cedar", "alloy", "coral"] = "marin"


def live_payload(offer: LiveOffer, backend: str) -> dict:
    return {
        "session": {
            "model": "gpt-live-1",
            "audio": {"output": {"voice": offer.voice}},
            "instructions": "You are an AI voice study companion. Keep replies concise, explain one idea at a time, and allow interruptions. You have no access to notebook sources. Delegate reasoning questions to the backend. Never claim to have read notebook documents.",
            "delegation": {
                "type": "responses",
                "responses": {
                    "model": backend,
                    "max_output_tokens": 4096,
                    "instructions": "Help with the spoken conversation. You have no notebook documents or source access. Be clear about this limitation when asked about a notebook.",
                },
            },
        },
        "transport": {"type": "webrtc", "sdp": offer.sdp},
    }


@router.post("/live/session")
async def create_live_session(offer: LiveOffer, request: Request):
    origin = request.headers.get("origin")
    if origin and urlsplit(origin).netloc != request.headers.get("host"):
        raise InvalidInputError("Start Live from the notebook application")
    if not offer.sdp.startswith("v=0"):
        raise InvalidInputError("Invalid WebRTC offer")
    defaults = await DefaultModels.get_instance()
    backend = None
    for model_id in dict.fromkeys(
        [defaults.default_chat_model, defaults.default_transformation_model]
    ):
        if not model_id:
            continue
        candidate = await Model.get(model_id)
        if (
            candidate.provider == "openai"
            and candidate.type == "language"
            and candidate.name.startswith(("gpt-6", "gpt-5", "gpt-4"))
        ):
            backend = candidate
            break
    if backend is None:
        raise ConfigurationError(
            "Live requires an OpenAI chat or transformation backend, such as GPT-6.1 Sol"
        )
    credential = await backend.get_credential_obj()
    if not credential:
        raise ConfigurationError("Link the OpenAI chat model to a credential first")
    config = credential.to_esperanto_config()
    key = config.get("api_key")
    if not key:
        raise ConfigurationError("OpenAI credential has no API key")
    if (
        config.get("base_url")
        and str(config["base_url"]).rstrip("/") != "https://api.openai.com/v1"
    ):
        raise ConfigurationError("Live requires a direct OpenAI API credential")
    try:
        async with httpx.AsyncClient(timeout=45) as client:
            result = await client.post(
                "https://api.openai.com/v1/live/sessions",
                headers={"Authorization": f"Bearer {key}"},
                json=live_payload(offer, backend.name),
            )
        if result.status_code >= 400:
            raise ExternalServiceError(
                f"OpenAI Live rejected session creation (HTTP {result.status_code}). Check Live access and API credit balance."
            )
        data = result.json()
        return {
            "session": {"id": data["session"]["id"]},
            "transport": {"type": "webrtc", "sdp": data["transport"]["sdp"]},
        }
    except httpx.HTTPError as error:
        raise ExternalServiceError("Could not connect to OpenAI Live") from error
    except (KeyError, ValueError) as error:
        raise ExternalServiceError("OpenAI returned an invalid Live session") from error
