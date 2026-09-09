#!/usr/bin/env python3
"""Tests for the gate ladder's book handling (the 2026-09-09 repeat bug guard).

The first ladder reused one 12-position book for every batch, so each batch replayed the
same games and the SPRT counted dependent repeats. These tests pin the fix: every batch
gets a disjoint slice, a repeat is refused, and an exhausted book stops the run instead of
wrapping around.

Standalone: `python benchmarks/scripts/test_gate_ladder.py`, or via pytest.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gate_ladder as gl  # noqa: E402

BOOK = """\
r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq -
r1bqkb1r/pppp1ppp/2n2n2/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq -
rnbqkbnr/pp2pppp/3p4/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq -
# a comment line is skipped
r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 1
"""


def _write_book(tmp: Path) -> Path:
    p = tmp / "book.epd"
    p.write_text(BOOK, encoding="utf-8")
    return p


def test_load_book_dedupes_on_four_fields_and_skips_comments(tmp_path: Path):
    book = gl.load_book(_write_book(tmp_path))
    # the last line repeats the first position with clocks -> dropped
    assert len(book) == 3, book
    assert book[0] == "r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq -"


def test_take_slice_is_disjoint_and_advances(tmp_path: Path):
    book = gl.load_book(_write_book(tmp_path))
    used: set[str] = set()
    a, cursor = gl.take_slice(book, 0, 2, used)
    b, cursor = gl.take_slice(book, cursor, 2, used)
    assert a == book[:2]
    assert b == book[2:3], "short tail allowed, wrapping not"
    assert cursor == 3
    assert not (set(a) & set(b))


def test_take_slice_refuses_a_repeat(tmp_path: Path):
    book = gl.load_book(_write_book(tmp_path))
    used = {book[0]}
    try:
        gl.take_slice(book, 0, 2, used)
        raise AssertionError("expected a repeat refusal")
    except SystemExit as exc:
        assert "repeat" in str(exc)


def test_exhausted_book_returns_empty_slice(tmp_path: Path):
    book = gl.load_book(_write_book(tmp_path))
    used: set[str] = set()
    _, cursor = gl.take_slice(book, 0, 3, used)
    empty, cursor2 = gl.take_slice(book, cursor, 2, used)
    assert empty == []
    assert cursor2 == cursor == 3


def test_cli_requires_a_book():
    try:
        gl.main(["--out-dir", "x", "--exe", "y"])
        raise AssertionError("expected SystemExit for a missing --book")
    except SystemExit:
        pass


if __name__ == "__main__":
    import tempfile

    failures = 0
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        for name, fn in sorted(globals().items()):
            if not name.startswith("test_") or not callable(fn):
                continue
            try:
                if fn.__code__.co_argcount:
                    fn(tmp)
                else:
                    fn()
                print(f"ok   {name}")
            except Exception as exc:  # noqa: BLE001
                failures += 1
                print(f"FAIL {name}: {exc!r}")
    raise SystemExit(1 if failures else 0)
