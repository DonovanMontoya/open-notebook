from datetime import datetime, timedelta, timezone
from typing import Any

from fastapi import APIRouter, Query

from open_notebook.database.repository import repo_query

router = APIRouter()


@router.get("/usage")
async def get_usage(days: int = Query(default=30, ge=1, le=365)):
    since = datetime.now(timezone.utc) - timedelta(days=days)
    rows = await repo_query(
        "SELECT * FROM usage_event WHERE created >= $since ORDER BY created DESC",
        {"since": since},
    )
    groups: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        key = (row["model"], row["feature"])
        group = groups.setdefault(
            key,
            {
                "model": key[0],
                "feature": key[1],
                "calls": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "audio_seconds": 0,
                "estimated_usd": 0,
                "unpriced": 0,
            },
        )
        group["calls"] += 1
        for field in (
            "input_tokens",
            "output_tokens",
            "audio_seconds",
            "estimated_usd",
        ):
            group[field] += row.get(field) or 0
        group["unpriced"] += (
            row.get("estimated_usd") is None or row.get("basis") == "usage_unavailable"
        )
    return {
        "days": days,
        "estimated_usd": sum(g["estimated_usd"] for g in groups.values()),
        "calls": len(rows),
        "unpriced": sum(g["unpriced"] for g in groups.values()),
        "groups": list(groups.values()),
        "recent": rows[:100],
        "pricing_date": "2026-10-31",
    }
