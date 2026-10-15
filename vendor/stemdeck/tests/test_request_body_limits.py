"""Unbounded request input (#512).

The body-size guard was scoped to paths ending /sections or /beats, so every
other JSON endpoint accumulated an arbitrarily large body and then ran
json.loads on the event loop. A chunked request skipped the check entirely.

The trim range had no ceiling either: `end` reaches
np.zeros(int(round(duration * sample_rate))) in the click renderer, so
?start=0&end=20000&count_in=1 asked for a multi-GB allocation.
"""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from app.core.models import Job
from app.core.registry import _jobs
from app.core.registry import register as registry_register


@pytest.fixture(autouse=True)
def _clean_jobs():
    _jobs.clear()
    yield
    _jobs.clear()


@pytest.fixture
def client():
    from app.main import app

    with TestClient(app) as c:
        yield c


@pytest.fixture
def big_body():
    from app.main import _JSON_BODY_LIMIT

    return "x" * (_JSON_BODY_LIMIT + 1024)


@pytest.mark.parametrize(
    "path",
    [
        "/api/search",
        "/api/playlist",
        "/api/playlist/preview",
        "/api/settings",
    ],
)
def test_a_huge_json_body_is_refused_before_it_is_parsed(client, path, big_body):
    # Previously uncapped: Starlette buffers the whole body, then json.loads
    # runs it on the event loop and stalls every other request.
    res = client.post(
        path, content=f'{{"q": "{big_body}"}}', headers={"content-type": "application/json"}
    )

    assert res.status_code == 413


def test_a_chunked_body_cannot_skip_the_check(client):
    # No Content-Length made `declared` None, so the guard fell through to an
    # unbounded request.body().
    res = client.post(
        "/api/search",
        content=iter([b'{"q": "', b"x" * 4096, b'"}']),
        headers={"content-type": "application/json", "transfer-encoding": "chunked"},
    )

    assert res.status_code == 411


def test_a_normal_json_body_still_works(client):
    res = client.post("/api/settings", json={"max_duration_sec": 600})

    assert res.status_code == 200


# ─── trim range ───


def _done_job(job_id="a1b2c3d4e5f6", duration=180.0):
    job = Job(id=job_id, status="done", title="Song", duration_sec=duration)
    registry_register(job)
    return job


def test_a_trim_end_beyond_the_track_is_refused(client, tmp_path):
    _done_job()

    res = client.get(
        "/api/jobs/a1b2c3d4e5f6/mixdown.wav",
        params={"stems": "vocals", "gains": "1", "start": 0, "end": 20000, "count_in": 1},
    )

    assert res.status_code == 422, "an unbounded end reaches a multi-GB np.zeros"


def test_a_trim_range_inside_the_track_is_not_refused_by_the_bound(client):
    # Must not 422 on the bound; a later 404 for missing stems is fine.
    _done_job()

    res = client.get(
        "/api/jobs/a1b2c3d4e5f6/mixdown.wav",
        params={"stems": "vocals", "gains": "1", "start": 0, "end": 120},
    )

    assert res.status_code != 422
