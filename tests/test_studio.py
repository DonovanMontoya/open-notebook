import json
from types import SimpleNamespace
from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from api.routers.studio import StudyRequest, generate_study_deck, parse_deck
from open_notebook.exceptions import IncompleteGenerationError, InvalidInputError


def payload(kind="flashcards"):
    item: dict[str, Any] = {
        "question": "What is retrieval?",
        "answer": "Finding relevant source material",
        "source_ids": ["source:one"],
    }
    if kind == "quiz":
        item.update(
            options=["Retrieval", "Training", "Deletion", "Compression"],
            correct_index=0,
        )
    return {"version": 1, "kind": kind, "title": "Retrieval", "items": [item]}


@pytest.mark.parametrize("kind", ["flashcards", "quiz"])
def test_valid_deck(kind):
    request = StudyRequest(kind=kind, source_ids=["source:one"], count=1)
    assert parse_deck(json.dumps(payload(kind)), request).kind == kind


@pytest.mark.parametrize("failure", ["citation", "count", "choices", "index", "json"])
def test_invalid_generated_content(failure):
    request = StudyRequest(kind="quiz", source_ids=["source:one"], count=1)
    data = payload("quiz")
    if failure == "citation":
        data["items"][0]["source_ids"] = ["source:invented"]
    if failure == "count":
        data["items"] *= 2
    if failure == "choices":
        data["items"][0]["options"] = ["same"] * 4
    if failure == "index":
        data["items"][0]["correct_index"] = 4
    with pytest.raises(IncompleteGenerationError):
        parse_deck("not JSON" if failure == "json" else json.dumps(data), request)


@pytest.mark.asyncio
async def test_rejects_foreign_sources_before_ai():
    notebook = SimpleNamespace(get_sources=AsyncMock(return_value=[]))
    with (
        patch("api.routers.studio.Notebook.get", AsyncMock(return_value=notebook)),
        patch("api.routers.studio.run_transformation", AsyncMock()) as run,
    ):
        with pytest.raises(InvalidInputError):
            await generate_study_deck(
                "notebook:one", StudyRequest(kind="quiz", source_ids=["source:foreign"])
            )
        run.assert_not_called()


@pytest.mark.asyncio
async def test_generated_deck_is_saved_and_linked():
    source = SimpleNamespace(
        id="source:one",
        title="Source",
        full_text="Retrieval finds relevant source material.",
    )
    notebook = SimpleNamespace(get_sources=AsyncMock(return_value=[source]))
    with (
        patch("api.routers.studio.Notebook.get", AsyncMock(return_value=notebook)),
        patch(
            "api.routers.studio.run_transformation",
            AsyncMock(return_value={"output": json.dumps(payload())}),
        ),
        patch("api.routers.studio.Note.save", AsyncMock()) as save,
        patch("api.routers.studio.Note.add_to_notebook", AsyncMock()) as link,
    ):
        deck = await generate_study_deck(
            "notebook:one",
            StudyRequest(kind="flashcards", source_ids=["source:one"], count=1),
        )
        assert deck.title == "Retrieval"
        save.assert_awaited_once()
        link.assert_awaited_once_with("notebook:one")
