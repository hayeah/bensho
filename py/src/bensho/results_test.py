"""Results over a synthetic frame with a known nested order."""

from pathlib import Path

import pandas as pd
import pytest

from bensho.results import BENSHO_COLUMNS, EmptyDataFrameError, Results, cell_path

# One suite, two groups of two cells and one singleton, three rounds. Per
# round: the group order, then each group's inner order. ``large`` is
# Budget-calibrated; ``small/vec_sum`` is noisy.
ROUNDS = {
    1: [
        ("small", ["vec_sum", "fold_sum"]),
        ("large", ["fold_sum", "vec_sum"]),
        ("", ["baseline"]),
    ],
    2: [
        ("", ["baseline"]),
        ("small", ["fold_sum", "vec_sum"]),
        ("large", ["vec_sum", "fold_sum"]),
    ],
    3: [
        ("large", ["vec_sum", "fold_sum"]),
        ("", ["baseline"]),
        ("small", ["vec_sum", "fold_sum"]),
    ],
}
NS = {
    ("small", "vec_sum"): [10.0, 12.0, 20.0],
    ("small", "fold_sum"): [11.0, 11.0, 11.0],
    ("large", "vec_sum"): [50000.0, 50500.0, 49500.0],
    ("large", "fold_sum"): [51000.0, 51000.0, 51000.0],
    ("", "baseline"): [0.3, 0.3, 0.3],
}


def rows() -> list[dict[str, object]]:
    out = []
    for rnd, order in ROUNDS.items():
        position = 0
        for group_position, (group, names) in enumerate(order):
            for name in names:
                ns = NS[(group, name)][rnd - 1]
                calibrated = group == "large"
                batch = 40 if calibrated else 1000
                out.append(
                    {
                        "suite": "toy",
                        "group": group,
                        "name": name,
                        "round": rnd,
                        "position": position,
                        "group_position": group_position,
                        "cells": 5,
                        "seed": 7,
                        "batch": batch,
                        "ops": batch,
                        "elapsed_ns": int(ns * batch),
                        "ns_per_op": ns,
                        "calibration": "Budget" if calibrated else "Full",
                        "pilot_ns_per_op": ns * 1.5,
                        "start_ms": rnd * 1000 + position * 100,
                        "data.subject": name,
                        "data.bytes": {"small": 8000, "large": 8_000_000}.get(group, 0),
                    }
                )
                position += 1
    return out


def frame() -> pd.DataFrame:
    return pd.DataFrame(rows())


def write_tree(root: Path) -> None:
    """The frame as the harness would have written it: one file per cell."""
    df = frame()
    for (group, name), d in df.groupby(["group", "name"], sort=False):
        f = root / "toy" / f"{cell_path(str(group), str(name))}.csv"
        f.parent.mkdir(parents=True, exist_ok=True)
        d.to_csv(f, index=False)


def test_columns_user_columns_and_paths() -> None:
    r = Results(frame())
    assert list(r.df.columns[: len(BENSHO_COLUMNS)]) == BENSHO_COLUMNS
    assert r.user_columns == ["data.subject", "data.bytes"]
    assert r.suites == ["toy"]
    assert r.groups == ["small", "large", ""]
    assert r.paths == [
        "small/vec_sum",
        "small/fold_sum",
        "large/fold_sum",
        "large/vec_sum",
        "baseline",
    ]


def test_rejects_non_bensho_frames() -> None:
    with pytest.raises(ValueError, match="missing columns"):
        Results(pd.DataFrame({"subject": ["a"]}))
    with pytest.raises(EmptyDataFrameError):
        Results(frame().iloc[:0])


def test_load_directory_keeps_empty_group_and_relative_source(tmp_path: Path) -> None:
    write_tree(tmp_path)
    r = Results.load([tmp_path])
    assert len(r.df) == 15
    assert sorted(set(r.df["source"])) == [
        "toy/baseline.csv",
        "toy/large/fold_sum.csv",
        "toy/large/vec_sum.csv",
        "toy/small/fold_sum.csv",
        "toy/small/vec_sum.csv",
    ]
    base = r.df[r.df["name"] == "baseline"]
    assert (base["group"] == "").all(), "empty group reads back as '', not NaN"
    assert (base["path"] == "baseline").all()
    # a suite directory: sources relative to it
    r2 = Results.load([tmp_path / "toy"])
    assert "baseline.csv" in set(r2.df["source"])
    # a single file
    r3 = Results.load([tmp_path / "toy" / "small" / "vec_sum.csv"])
    assert set(r3.df["source"]) == {"vec_sum.csv"}


def test_census_is_per_file(tmp_path: Path) -> None:
    write_tree(tmp_path)
    c = Results.load([tmp_path]).census()
    assert list(c.columns) == [
        "suite",
        "group",
        "name",
        "rows",
        "rounds",
        "expected",
        "missing",
        "seeds",
        "cells",
    ]
    assert c.loc["toy/small/vec_sum.csv", "group"] == "small"
    assert (c["rows"] == 3).all()
    assert (c["missing"] == 0).all()
    assert (c["seeds"] == "7").all()
    # drop the last round of one cell: it is missing one against the suite's 3
    dropped = Results(frame().iloc[:-1]).census()
    assert dropped.loc["", "missing"] == 1  # one source, ""


def test_cells_keyed_by_suite_group_name_and_flags_calibrated() -> None:
    cells = Results(frame()).cells()
    assert cells.index.names == ["suite", "group", "name"]
    assert cells.loc[("toy", "large", "vec_sum"), "calibrated"]
    assert not cells.loc[("toy", "small", "vec_sum"), "calibrated"]
    assert cells.loc[("toy", "small", "vec_sum"), "min"] == 10.0
    assert cells.loc[("toy", "small", "vec_sum"), "median"] == 12.0
    assert cells.loc[("toy", "large", "vec_sum"), "batch"] == 40
    assert cells.loc[("toy", "", "baseline"), "rounds"] == 3


def test_anomalies_lists_calibrated_and_noisy() -> None:
    a = Results(frame()).anomalies().set_index(["suite", "group", "name"])
    assert "Budget" in str(a.loc[("toy", "large", "vec_sum"), "note"])
    assert "CV" in str(a.loc[("toy", "small", "vec_sum"), "note"])
    assert ("toy", "small", "fold_sum") not in a.index


def test_normalized_slots_at_three_levels() -> None:
    d = Results(frame()).normalized().set_index(["round", "group", "name"])
    # round 1: small(vec, fold) large(fold, vec) baseline
    assert d.loc[(1, "small", "vec_sum"), "slot_round"] == 0.0
    assert d.loc[(1, "", "baseline"), "slot_round"] == 1.0
    assert d.loc[(1, "small", "vec_sum"), "slot_group"] == 0.0
    assert d.loc[(1, "large", "vec_sum"), "slot_group"] == 0.5
    assert d.loc[(1, "", "baseline"), "slot_group"] == 1.0
    assert d.loc[(1, "small", "vec_sum"), "slot_cell"] == 0.0
    assert d.loc[(1, "small", "fold_sum"), "slot_cell"] == 1.0
    assert d.loc[(1, "large", "fold_sum"), "slot_cell"] == 0.0
    assert pd.isna(d.loc[(1, "", "baseline"), "slot_cell"])
    assert d.loc[(2, "small", "fold_sum"), "rank"] == 0
    assert d.loc[(2, "small", "vec_sum"), "rank"] == 1
    assert (d["unit"].loc[(slice(None), "", "baseline")] == "baseline").all()


def test_position_drift_at_each_level() -> None:
    r = Results(frame())
    rd = r.round_drift()
    assert list(rd.index) == [1, 2, 3]
    assert (rd["cells"] == 5).all()
    for level in ("round", "group"):
        p = r.position_drift(level)  # type: ignore[arg-type]
        assert list(p.index) == ["(all)", "small", "large", "(singletons)"]
        assert p.loc["(all)", "rows"] == 15
    p = r.position_drift("cell")
    assert p.loc["(all)", "rows"] == 12, "singletons contribute no cell slot"
    assert p.loc["(singletons)", "rows"] == 0


def test_carryover_at_each_level() -> None:
    r = Results(frame())
    co = r.carryover("round")
    assert co.loc[("small/vec_sum", "(first)"), "rows"] == 1
    assert co.loc[("large/fold_sum", "small/fold_sum"), "rows"] == 1
    assert co.loc[("baseline", "large/vec_sum"), "rows"] == 1

    co = r.carryover("group")
    # small opens round 1; baseline opens round 2; large opens round 3
    assert co.loc[("small", "(first)"), "rows"] == 2
    assert co.loc[("baseline", "(first)"), "rows"] == 1
    assert co.loc[("large", "(first)"), "rows"] == 2
    assert co.loc[("large", "small"), "rows"] == 4, "two visits x two cells"
    assert co.loc[("small", "baseline"), "rows"] == 4

    co = r.carryover("cell")
    # vec_sum runs first on the fresh small state in rounds 1 and 3
    assert co.loc[("small/vec_sum", "(first)"), "rows"] == 2
    assert co.loc[("small/vec_sum", "fold_sum"), "rows"] == 1
    assert co.loc[("small/fold_sum", "vec_sum"), "rows"] == 2
    assert co.loc[("baseline", "(first)"), "rows"] == 3


def test_tables_dump_and_figure(tmp_path: Path) -> None:
    r = Results(frame())
    names = list(r.tables())
    assert names == [
        "census",
        "cells",
        "anomalies",
        "round_drift",
        "position_drift_round",
        "position_drift_group",
        "position_drift_cell",
        "carryover_round",
        "carryover_group",
        "carryover_cell",
    ]
    r.dump(tmp_path / "out")
    assert (tmp_path / "out" / "carryover_cell.tsv").exists()
    for color_by in (None, "group", "data.subject"):
        fig = r.figure(color_by=color_by)
        fig.savefig(tmp_path / "fig.png")
        assert (tmp_path / "fig.png").stat().st_size > 0
