"""The shared SSE connection budget must not leak (#513).

claim_sse_slot() runs in the handler so hitting the cap can still answer 503.
release lives in the stream's finally -- and an async generator that is never
started never runs its finally. A client that disconnects before the response
body begins therefore held a slot forever: 200 of those and every progress
stream 503s with nothing actually connected, until the process restarts.
"""

from __future__ import annotations

import gc

import pytest
from fastapi import HTTPException

from app.api import events as _events


@pytest.fixture(autouse=True)
def _zero_budget():
    _events._sse_active = 0
    yield
    _events._sse_active = 0


def test_a_slot_is_held_then_released():
    slot = _events.SseSlot()
    assert _events._sse_active == 1

    slot.release()
    assert _events._sse_active == 0


def test_release_is_idempotent():
    # The stream's finally and the __del__ backstop can both fire; counting
    # twice would free a slot that is still in use.
    slot = _events.SseSlot()
    slot.release()
    slot.release()

    assert _events._sse_active == 0


def test_a_slot_dropped_without_release_is_reclaimed():
    # The leak: the generator is created, so the slot is claimed, but never
    # iterated, so its finally never runs. Collecting it must free the slot.
    def _never_started():
        slot = _events.SseSlot()

        async def stream():
            try:
                yield "data: x\n\n"
            finally:
                slot.release()

        return stream()  # created, never iterated

    gen = _never_started()
    assert _events._sse_active == 1

    del gen
    gc.collect()

    assert _events._sse_active == 0, "a slot leaked for the life of the process"


def test_the_cap_still_answers_503():
    held = [_events.SseSlot() for _ in range(_events._MAX_SSE_CONNECTIONS)]
    assert _events._sse_active == _events._MAX_SSE_CONNECTIONS

    with pytest.raises(HTTPException) as excinfo:
        _events.SseSlot()
    assert excinfo.value.status_code == 503

    for slot in held:
        slot.release()
    assert _events._sse_active == 0


def test_a_refused_claim_holds_nothing():
    # If __init__ raises, no slot was taken -- so the failed attempt must not
    # decrement on collection either.
    held = [_events.SseSlot() for _ in range(_events._MAX_SSE_CONNECTIONS)]
    with pytest.raises(HTTPException):
        _events.SseSlot()

    gc.collect()
    assert _events._sse_active == _events._MAX_SSE_CONNECTIONS

    for slot in held:
        slot.release()
