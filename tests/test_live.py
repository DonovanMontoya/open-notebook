from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import httpx
import pytest
from starlette.requests import Request

from api.routers.live import LiveOffer, create_live_session, live_payload
from open_notebook.exceptions import ExternalServiceError, InvalidInputError


def request(origin="https://notebook.example"):
    return Request(
        {
            "type": "http",
            "headers": [(b"origin", origin.encode()), (b"host", b"notebook.example")],
        }
    )


def test_payload_keeps_backend_and_voice_separate():
    offer = LiveOffer(sdp="v=0\r\n" + "x" * 30)
    payload = live_payload(offer, "gpt-6.1-sol")
    assert payload["session"]["model"] == "gpt-live-1"
    assert payload["session"]["delegation"]["responses"]["model"] == "gpt-6.1-sol"
    assert (
        "no notebook"
        in payload["session"]["delegation"]["responses"]["instructions"].lower()
    )
    assert payload["transport"]["sdp"] == offer.sdp


@pytest.mark.asyncio
async def test_cross_origin_rejected_before_credentials():
    with pytest.raises(InvalidInputError):
        await create_live_session(
            LiveOffer(sdp="v=0\r\n" + "x" * 30), request("https://other.example")
        )


@pytest.mark.asyncio
@pytest.mark.parametrize("status", [201, 401])
async def test_exchange_never_returns_secret_or_provider_error(status):
    secret = "private-test-key"
    credential = SimpleNamespace(to_esperanto_config=lambda: {"api_key": secret})
    backend = SimpleNamespace(
        provider="openai",
        type="language",
        name="gpt-6.1-sol",
        get_credential_obj=AsyncMock(return_value=credential),
    )
    response = httpx.Response(
        status,
        json={
            "session": {"id": "live_test", "secret": secret},
            "transport": {"sdp": "answer"},
            "error": secret,
        },
    )
    with (
        patch(
            "api.routers.live.DefaultModels.get_instance",
            AsyncMock(
                return_value=SimpleNamespace(
                    default_chat_model="model:test", default_transformation_model=None
                )
            ),
        ),
        patch("api.routers.live.Model.get", AsyncMock(return_value=backend)),
        patch("httpx.AsyncClient.post", AsyncMock(return_value=response)),
    ):
        if status == 201:
            result = await create_live_session(
                LiveOffer(sdp="v=0\r\n" + "x" * 30), request()
            )
            assert secret not in str(result)
            assert result["transport"]["sdp"] == "answer"
        else:
            with pytest.raises(ExternalServiceError) as error:
                await create_live_session(
                    LiveOffer(sdp="v=0\r\n" + "x" * 30), request()
                )
            assert secret not in str(error.value)


@pytest.mark.asyncio
async def test_live_uses_openai_transformation_when_chat_is_anthropic():
    from open_notebook.exceptions import ConfigurationError

    chat = SimpleNamespace(provider="anthropic", type="language", name="claude")
    backend = SimpleNamespace(
        provider="openai",
        type="language",
        name="gpt-6.1-sol",
        get_credential_obj=AsyncMock(return_value=None),
    )
    with (
        patch(
            "api.routers.live.DefaultModels.get_instance",
            AsyncMock(
                return_value=SimpleNamespace(
                    default_chat_model="model:chat",
                    default_transformation_model="model:sol",
                )
            ),
        ),
        patch("api.routers.live.Model.get", AsyncMock(side_effect=[chat, backend])),
    ):
        with pytest.raises(ConfigurationError, match="Link the OpenAI"):
            await create_live_session(LiveOffer(sdp="v=0\r\n" + "x" * 30), request())
    backend.get_credential_obj.assert_awaited_once()
