from unittest.mock import patch

from fastapi import FastAPI
from fastapi.testclient import TestClient

from api.auth import PasswordAuthMiddleware
from api.podcast_media import create_media_ticket, valid_media_ticket

PATH = "/api/podcasts/episodes/podcast_episode:test/audio"


def test_media_ticket_is_scoped_expiring_and_tamper_proof():
    with patch("api.podcast_media.time.time", return_value=100):
        ticket = create_media_ticket(PATH)
        assert valid_media_ticket(PATH, ticket)
        assert not valid_media_ticket(PATH + "other", ticket)
        assert not valid_media_ticket("/api/models", ticket)
        assert not valid_media_ticket(PATH, ticket + "0")
        assert not valid_media_ticket(PATH, "invalid")
    with patch("api.podcast_media.time.time", return_value=86501):
        assert not valid_media_ticket(PATH, ticket)


def test_media_ticket_does_not_authorize_other_routes_or_writes(monkeypatch):
    monkeypatch.setenv("OPEN_NOTEBOOK_PASSWORD", "test-password")
    app = FastAPI()
    app.add_middleware(PasswordAuthMiddleware)

    @app.api_route(PATH, methods=["GET", "POST", "HEAD"])
    def audio():
        return {"ok": True}

    @app.get("/api/models")
    def models():
        return []

    client = TestClient(app)
    ticket = create_media_ticket(PATH)
    assert client.get(PATH).status_code == 401
    assert client.get(PATH, params={"media_ticket": ticket}).status_code == 200
    assert client.head(PATH, params={"media_ticket": ticket}).status_code == 200
    assert client.post(PATH, params={"media_ticket": ticket}).status_code == 401
    assert client.get("/api/models", params={"media_ticket": ticket}).status_code == 401
