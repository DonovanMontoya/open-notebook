from types import SimpleNamespace
from unittest.mock import AsyncMock, patch
from uuid import uuid4

import pytest

from api.routers.usage import get_usage
from open_notebook.ai.usage import UsageCallback, estimate_cost, record_usage


def test_estimates_and_unknown_models():
    assert estimate_cost("gpt-6.1-sol", 1000000, 1000000) == 12
    assert estimate_cost("gpt-6.1-sol", 1000000, 0, cached=1000000) == 0.1
    assert estimate_cost("not-priced", 100, 100) is None
    assert estimate_cost("gpt-live-1", 0, 0, seconds=60) == 0.05


@pytest.mark.asyncio
async def test_callback_stores_quantities_not_content():
    msg = SimpleNamespace(
        usage_metadata={"input_tokens": 100, "output_tokens": 20},
        response_metadata={"model_name": "gpt-6.1-sol"},
        content="private source text",
    )
    response = SimpleNamespace(generations=[[SimpleNamespace(message=msg)]])
    with patch("open_notebook.ai.usage.repo_upsert", new_callable=AsyncMock) as write:
        await UsageCallback("chat").on_llm_end(response, run_id=uuid4())
        data = write.call_args.args[2]
        assert data["input_tokens"] == 100
        assert data["estimated_usd"] == pytest.approx(0.0004)
        assert "private source" not in str(data)


@pytest.mark.asyncio
async def test_missing_usage_is_unpriced_and_database_failure_is_nonfatal():
    with patch("open_notebook.ai.usage.repo_upsert", new_callable=AsyncMock) as write:
        await record_usage(
            model="gpt-6.1-sol", feature="chat", basis="usage_unavailable"
        )
        assert write.call_args.args[2]["estimated_usd"] is None
    with patch(
        "open_notebook.ai.usage.repo_upsert",
        AsyncMock(side_effect=RuntimeError("offline")),
    ):
        await record_usage(model="gpt-6.1-sol", feature="chat")


@pytest.mark.asyncio
async def test_summary_keeps_unpriced_visible():
    with patch(
        "api.routers.usage.repo_query",
        AsyncMock(
            return_value=[
                {"model": "known", "feature": "chat", "estimated_usd": 0.3},
                {"model": "unknown", "feature": "chat", "estimated_usd": None},
            ]
        ),
    ):
        result = await get_usage(30)
    assert result["estimated_usd"] == 0.3
    assert result["unpriced"] == 1
    assert result["calls"] == 2
