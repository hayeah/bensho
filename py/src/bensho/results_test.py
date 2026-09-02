"""Results over a synthetic frame with a known shape."""

from pathlib import Path

import pandas as pd
import pytest

from bensho.results import BENSHO_COLUMNS, EmptyDataFrameError, Results

# Two subjects x two modes, three rounds, a shuffled position per round, one
# user column. ``slow/large`` is Budget-calibrated; ``fast/small`` is noisy.
ORDER = {1: [0, 1, 2, 3], 2: [3, 2, 1, 0], 3: [1, 3, 0, 2]}
CELLS = [("fast", "small"), ("fast", "large"), ("slow", "small"), ("slow", "large")]
NS = {
    ("fast", "small"): [10.0, 12.0, 20.0],
    ("fast", "large"): [1000.0, 1010.0, 990.0],
    ("slow", "small"): [50.0, 51.0, 49.0],
    ("slow", "large"): [50000.0, 50500.0, 49500.0],
}


def frame() -> pd.DataFrame:
    rows = []
    for rnd, order in ORDER.items():
        for position, idx in enumerate(order):
            subject, mode = CELLS[idx]
            ns = NS[(subject, mode)][rnd - 1]
            calibrated = (subject, mode) == ("slow", "large")
            batch = 40 if calibrated else 1000
            rows.append(
                {
                    "subject": subject,
                    "mode": mode,
                    "round": rnd,
                    "position": position,
                    "cells": 4,
                    "seed": 7,
                    "batch": batch,
                    "ops": batch,
                    "elapsed_ns": int(ns * batch),
                    "ns_per_op": ns,
                    "calibration": "Budget" if calibrated else "Full",
                    "pilot_ns_per_op": ns * 1.5,
                    "start_ms": rnd * 1000 + position * 100,
                    "bytes": 8 if mode == "small" else 8000,
                }
            )
    return pd.DataFrame(rows)


def test_columns_and_user_columns() -> None:
    r = Results(frame())
    assert list(r.df.columns[: len(BENSHO_COLUMNS)]) == BENSHO_COLUMNS
    assert r.user_columns == ["bytes"]
    assert r.subjects == ["fast", "slow"]
    assert r.modes == ["small", "large"]


def test_rejects_non_bensho_frames() -> None:
    with pytest.raises(ValueError, match="missing columns"):
        Results(pd.DataFrame({"subject": ["a"]}))
    with pytest.raises(EmptyDataFrameError):
        Results(frame().iloc[:0])


def test_census_counts_expected_rows() -> None:
    c = Results(frame()).census()
    assert c.loc["", "rows"] == 12
    assert c.loc["", "expected"] == 12
    assert c.loc["", "missing"] == 0
    dropped = Results(frame().iloc[:-1]).census()
    assert dropped.loc["", "missing"] == 1


def test_cells_flags_calibrated() -> None:
    cells = Results(frame()).cells()
    assert cells.loc[("slow", "large"), "calibrated"]
    assert not cells.loc[("fast", "small"), "calibrated"]
    assert cells.loc[("fast", "small"), "min"] == 10.0
    assert cells.loc[("fast", "small"), "median"] == 12.0
    assert cells.loc[("slow", "large"), "batch"] == 40


def test_anomalies_lists_calibrated_and_noisy() -> None:
    a = Results(frame()).anomalies().set_index(["subject", "mode"])
    assert "Budget" in str(a.loc[("slow", "large"), "note"])
    assert "CV" in str(a.loc[("fast", "small"), "note"])
    assert ("fast", "large") not in a.index


def test_drift_tables_have_the_rounds_and_subjects() -> None:
    r = Results(frame())
    rd = r.round_drift()
    assert list(rd.index) == [1, 2, 3]
    assert (rd["cells"] == 4).all()
    pdrift = r.position_drift()
    assert list(pdrift.index) == ["(all)", "fast", "slow"]
    assert pdrift.loc["(all)", "rows"] == 12
    co = r.carryover()
    # round 1 opens with fast/small; every subject has a "(first)" row somewhere
    assert ("fast", "(first)") in co.index
    assert ("slow", "(first)") in co.index


def test_load_and_dump(tmp_path: Path) -> None:
    csv = tmp_path / "a.csv"
    frame().to_csv(csv, index=False)
    r = Results.load([csv, csv])
    assert set(r.df["source"]) == {"a.csv"}
    assert len(r.df) == 24
    r.dump(tmp_path / "out")
    assert (tmp_path / "out" / "cells.tsv").exists()
    fig = r.figure()
    fig.savefig(tmp_path / "fig.png")
    assert (tmp_path / "fig.png").stat().st_size > 0
