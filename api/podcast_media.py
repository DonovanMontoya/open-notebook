"""Scoped media tickets let native browser audio make authenticated Range requests."""

import hashlib
import hmac
import re
import secrets
import time

from open_notebook.utils.encryption import get_secret_from_env

_PROCESS_KEY = secrets.token_hex(32)
_AUDIO_PATH = re.compile(r"^/api/podcasts/episodes/[^/]+/audio$")


def _signature(path: str, expiry: str) -> str:
    key = get_secret_from_env("OPEN_NOTEBOOK_PASSWORD") or _PROCESS_KEY
    return hmac.new(
        key.encode(), f"podcast-media:{path}:{expiry}".encode(), hashlib.sha256
    ).hexdigest()


def create_media_ticket(path: str) -> str:
    expiry = str(int(time.time()) + 86400)
    return f"{expiry}.{_signature(path, expiry)}"


def valid_media_ticket(path: str, ticket: str) -> bool:
    if not _AUDIO_PATH.fullmatch(path):
        return False
    try:
        expiry, signature = ticket.split(".", 1)
        if not int(time.time()) < int(expiry) <= int(time.time()) + 86400:
            return False
        return hmac.compare_digest(signature, _signature(path, expiry))
    except (ValueError, TypeError):
        return False
