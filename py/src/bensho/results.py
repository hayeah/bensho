"""The bensho read-out: one class over one DataFrame of harness rows.

The contract with the Rust side is the column list ``BENSHO_COLUMNS`` (see
``specs/20260902_182536_bensho_suite_csv_sink_and_filter.md``, "CSV
contract"). Every ``data.`` column is the bench's own and passes through
untouched as ``Results.user_columns``.

The harness writes one file per cell under ``<out>/<suite>/<group>/<cell>.csv``
and interleaves at two levels: groups within a round, cells within a group,
each group's state built fresh when the round reaches it. Three ideas follow:

- ``batch`` is per row because the harness calibrates the batch DOWN for
  slow cells to a time budget. ns/op stays comparable (it is a rate), but a
  small batch has fewer ops behind it, so calibrated cells are flagged in
  ``cells()`` and listed in ``anomalies()`` rather than silently averaged in.
- The recorded ``position`` and ``group_position`` reconstruct the order, so
  the drift tables normalise every row to its cell's median and ask whether
  the round, the slot, or the predecessor moved it, at whichever level.
- Within a round a group's cells share the state earlier cells left behind;
  ``carryover("cell")`` and ``position_drift("cell")`` measure that.
"""

from __future__ import annotations

from collections.abc import Iterable
from pathlib import Path
from typing import Literal

import numpy as np
import pandas as pd

#: The harness's columns, in CSV order.
BENSHO_COLUMNS = [
    "suite",
    "group",
    "name",
    "round",
    "position",
    "group_position",
    "cells",
    "seed",
    "batch",
    "ops",
    "elapsed_ns",
    "ns_per_op",
    "calibration",
    "pilot_ns_per_op",
    "start_ms",
]

#: A cell's identity across files.
KEY = ["suite", "group", "name"]

#: One round of one run of one suite: the rows that were interleaved.
ROUND = ["suite", "seed", "round"]

#: Where a slot or a predecessor is read: the flattened round, the groups
#: within the round, or the cells within a group.
Level = Literal["round", "group", "cell"]
LEVELS: tuple[Level, ...] = ("round", "group", "cell")

#: Coefficient of variation above which a cell is listed in ``anomalies()``.
CV_FLAG_PCT = 5.0

#: Categorical palette, fixed first-seen order, never cycled.
PALETTE = [
    "#2a78d6",
    "#eb6834",
    "#1baf7a",
    "#eda100",
    "#e87ba4",
    "#008300",
    "#4a3aa7",
    "#e34948",
]
SURFACE = "#fcfcfb"
TEXT = "#0b0b0b"
TEXT2 = "#52514e"
GRID = "#e6e5e0"


class EmptyDataFrameError(ValueError):
    """Raised when an operation receives an unexpected empty DataFrame."""


def cell_path(group: str, name: str) -> str:
    """``<group>/<name>``, or ``<name>`` for a singleton."""
    return f"{group}/{name}" if group else name


class Results:
    """Everything the harness's CSVs support, as DataFrames."""

    def __init__(self, df: pd.DataFrame) -> None:
        missing = [c for c in BENSHO_COLUMNS if c not in df.columns]
        if missing:
            raise ValueError(f"not a bensho CSV, missing columns {missing}")
        if df.empty:
            raise EmptyDataFrameError("no rows")
        df = df.copy()
        for c in ("suite", "group", "name"):
            df[c] = df[c].fillna("").astype(str)
        if "source" not in df.columns:
            df["source"] = ""
        df["path"] = [
            cell_path(g, n) for g, n in zip(df["group"], df["name"], strict=True)
        ]
        self.df = df.reset_index(drop=True)

    @classmethod
    def load(cls, paths: Iterable[Path | str]) -> Results:
        """Concatenate files and directories (read recursively); ``source``
        is the file's path relative to the directory given, or its name."""
        frames = []
        for p in paths:
            p = Path(p)
            if p.is_dir():
                for f in sorted(p.rglob("*.csv")):
                    frames.append(cls._read(f, f.relative_to(p).as_posix()))
            else:
                frames.append(cls._read(p, p.name))
        if not frames:
            raise EmptyDataFrameError("no CSV paths")
        return cls(pd.concat(frames, ignore_index=True))

    @staticmethod
    def _read(path: Path, source: str) -> pd.DataFrame:
        df = pd.read_csv(path, dtype={"suite": str, "group": str, "name": str})
        return df.assign(source=source)

    # ---- shape --------------------------------------------------------------

    @property
    def user_columns(self) -> list[str]:
        """The bench's own columns, in CSV order."""
        return [c for c in self.df.columns if c.startswith("data.")]

    @property
    def suites(self) -> list[str]:
        return list(dict.fromkeys(self.df["suite"]))

    @property
    def groups(self) -> list[str]:
        """Group names in first-seen order; the empty string is the singletons."""
        return list(dict.fromkeys(self.df["group"]))

    @property
    def paths(self) -> list[str]:
        """Cell paths in first-seen order; the x axis of the figure."""
        return list(dict.fromkeys(self.df["path"]))

    def census(self) -> pd.DataFrame:
        """Per file: rows present against the rounds its suite reached (times
        the cells in the file, one for a harness-written file), and the
        distinct seeds and round sizes seen, so a directory assembled from
        several runs is reported rather than averaged silently."""
        reached = self.df.groupby("suite")["round"].max()
        rows = []
        for source, d in self.df.groupby("source", sort=False):
            suite = str(d["suite"].iloc[0])
            expected = int(reached[suite]) * d["path"].nunique()
            rows.append(
                {
                    "source": source,
                    "suite": suite,
                    "group": d["group"].iloc[0],
                    "name": d["name"].iloc[0],
                    "rows": len(d),
                    "rounds": int(d["round"].max()),
                    "expected": expected,
                    "missing": expected - len(d),
                    "seeds": ",".join(str(s) for s in dict.fromkeys(d["seed"])),
                    "cells": ",".join(str(c) for c in dict.fromkeys(d["cells"])),
                }
            )
        return pd.DataFrame(rows).set_index("source")

    # ---- per cell -----------------------------------------------------------

    def cells(self) -> pd.DataFrame:
        """Per (suite, group, name): round statistics of ns/op, batch, verdict."""
        g = self.df.groupby(KEY, sort=False)
        out = g.agg(
            rounds=("ns_per_op", "size"),
            min=("ns_per_op", "min"),
            median=("ns_per_op", "median"),
            max=("ns_per_op", "max"),
            mean=("ns_per_op", "mean"),
            std=("ns_per_op", "std"),
            batch=("batch", "max"),
            ops=("ops", "max"),
            calibration=("calibration", "first"),
            pilot_ns_per_op=("pilot_ns_per_op", "first"),
        )
        out["cv_pct"] = (100 * out["std"] / out["mean"]).round(2)
        out["calibrated"] = out["calibration"] != "Full"
        for c in ("min", "median", "max", "mean", "std", "pilot_ns_per_op"):
            out[c] = out[c].round(2)
        return out

    def anomalies(self) -> pd.DataFrame:
        """Cells worth a second look: calibrated batch, or CV above the flag."""
        flags = []
        for _, row in self.cells().reset_index().iterrows():
            why = []
            if row["calibrated"]:
                why.append(f"batch {row['calibration']} to {int(row['batch']):,}")
            if pd.notna(row["cv_pct"]) and row["cv_pct"] > CV_FLAG_PCT:
                why.append(f"CV {row['cv_pct']:.1f}%")
            if why:
                flags.append(
                    {
                        "suite": row["suite"],
                        "group": row["group"],
                        "name": row["name"],
                        "batch": int(row["batch"]),
                        "cv_pct": row["cv_pct"],
                        "note": "; ".join(why),
                    }
                )
        return pd.DataFrame(flags, columns=[*KEY, "batch", "cv_pct", "note"])

    # ---- drift, from the recorded positions ---------------------------------

    def normalized(self) -> pd.DataFrame:
        """The rows plus ``norm`` (ns/op over the cell's median), ``unit``
        (the group's identity: its name, or the cell's for a singleton),
        ``rank`` (the cell's index within its group's visit) and one slot
        column per level, each a fraction from 0 (first) to 1 (last):

        - ``slot_round``: ``position`` over the round.
        - ``slot_group``: ``group_position`` over the round's groups.
        - ``slot_cell``: ``rank`` over the group's cells; NaN for a group of one.
        """
        d = self.df.copy()
        med = d.groupby(["source", *KEY])["ns_per_op"].transform("median")
        d["norm"] = d["ns_per_op"] / med
        d["unit"] = np.where(d["group"] != "", d["group"], d["name"])
        visit = [*ROUND, "group_position"]
        d["rank"] = d["position"] - d.groupby(visit)["position"].transform("min")
        size = d.groupby(visit)["position"].transform("size")
        groups = d.groupby(ROUND)["group_position"].transform("nunique")
        d["slot_round"] = d["position"] / (d["cells"] - 1).clip(lower=1)
        d["slot_group"] = d["group_position"] / (groups - 1).clip(lower=1)
        d["slot_cell"] = (d["rank"] / (size - 1)).where(size > 1)
        return d

    def round_drift(self) -> pd.DataFrame:
        """Per round: the median and mean normalised ns/op across all cells.

        A trend here is the machine changing over the run (thermal, other
        load), not a subject."""
        d = self.normalized()
        out = d.groupby("round").agg(
            cells=("norm", "size"),
            median_norm=("norm", "median"),
            mean_norm=("norm", "mean"),
            start_ms=("start_ms", "min"),
        )
        return out.round(4)

    def position_drift(self, level: Level = "round") -> pd.DataFrame:
        """Does the slot a cell drew move its number?

        ``slope_pct`` is the fitted change in normalised ns/op from the first
        slot to the last, in percent; ``corr`` the Pearson correlation of
        ``norm`` with the slot. The slot is read at ``level``: the cell's
        slot in the flattened round, its group's slot among the round's
        groups, or its own slot among its group's cells (where groups of
        one contribute nothing). One row for all cells, then one per group.
        """
        d = self.normalized().dropna(subset=[f"slot_{level}"])
        rows = [self._slot_fit("(all)", d, level)]
        for g in self.groups:
            label = g or "(singletons)"
            rows.append(self._slot_fit(label, d[d["group"] == g], level))
        return pd.DataFrame(rows).set_index("group")

    @staticmethod
    def _slot_fit(name: str, d: pd.DataFrame, level: Level) -> dict[str, object]:
        x = d[f"slot_{level}"].to_numpy(float)
        y = d["norm"].to_numpy(float)
        if len(d) < 3 or np.ptp(x) == 0:
            return {"group": name, "rows": len(d), "slope_pct": np.nan, "corr": np.nan}
        slope, _ = np.polyfit(x, y, 1)
        corr = float(np.corrcoef(x, y)[0, 1]) if np.std(y) > 0 else np.nan
        return {
            "group": name,
            "rows": len(d),
            "slope_pct": round(100 * float(slope), 2),
            "corr": round(corr, 3) if pd.notna(corr) else np.nan,
        }

    def carryover(self, level: Level = "round") -> pd.DataFrame:
        """Mean normalised ns/op grouped by what ran immediately before.

        The reason the harness shuffles: a slow predecessor that consistently
        inflates its successor shows up here as a row above 1. At level
        ``round`` the subject is the cell path and the predecessor the cell
        that ran before it in the round; at ``group`` the subject is the
        group (or the singleton's cell) and the predecessor the previous
        group; at ``cell`` the subject is the cell path and the predecessor
        the previous cell of the same group's visit, so ``(first)`` is the
        cell that ran on the freshly built state."""
        d = self.normalized()
        if level == "round":
            subject, key, step = "path", ROUND, "position"
        elif level == "group":
            subject, key, step = "unit", ROUND, "group_position"
        else:
            subject, key, step = "path", [*ROUND, "group_position"], "rank"
        prev = d[[*key, step, subject]].copy()
        prev[step] = prev[step] + 1
        prev = prev.rename(columns={subject: "predecessor"})
        if level == "cell":
            prev["predecessor"] = d["name"].to_numpy()
        # one predecessor per slot: at level ``group`` every cell of the
        # previous group names the same predecessor
        prev = prev.drop_duplicates(subset=[*key, step])
        merged = d.merge(prev, on=[*key, step], how="left")
        merged["predecessor"] = merged["predecessor"].fillna("(first)")
        out = merged.groupby([subject, "predecessor"], sort=False).agg(
            rows=("norm", "size"), mean_norm=("norm", "mean")
        )
        return out.round(4)

    # ---- figure -------------------------------------------------------------

    def figure(self, color_by: str | None = None, title: str | None = None):
        """Median ns/op per cell with min-max whiskers: cell paths along x,
        one colour per distinct value of ``color_by`` (``group``, or a
        ``data.`` column) in first-seen order, or one colour for all; log y."""
        import matplotlib.pyplot as plt

        d = self.df
        colour_of = (
            d[color_by].astype(str) if color_by else pd.Series("", index=d.index)
        )
        keys = list(dict.fromkeys(colour_of))
        if len(keys) > len(PALETTE):
            raise ValueError(f"more than {len(PALETTE)} values of {color_by}; facet")
        colour = dict(zip(keys, PALETTE, strict=False))
        paths = self.paths
        stats = (
            d.assign(colour_key=colour_of)
            .groupby(["path", "colour_key"], sort=False)["ns_per_op"]
            .agg(["min", "median", "max"])
            .reset_index()
        )

        fig, ax = plt.subplots(figsize=(max(6, 0.9 * len(paths) + 3), 5), dpi=150)
        fig.patch.set_facecolor(SURFACE)
        ax.set_facecolor(SURFACE)
        for k in keys:
            s = stats[stats["colour_key"] == k]
            xs = [paths.index(p) for p in s["path"]]
            meds = s["median"].to_numpy(float)
            lo = meds - s["min"].to_numpy(float)
            hi = s["max"].to_numpy(float) - meds
            ax.errorbar(
                xs,
                meds,
                yerr=[lo, hi],
                fmt="o",
                color=colour[k],
                ecolor=colour[k],
                markersize=7,
                markeredgecolor=SURFACE,
                markeredgewidth=1.4,
                elinewidth=1.4,
                capsize=3,
                label=k or None,
                zorder=3,
            )
        ax.set_yscale("log")
        ax.set_xticks(range(len(paths)))
        ax.set_xticklabels(paths, color=TEXT2, rotation=30, ha="right")
        ax.set_ylabel("ns per op (median; whiskers min to max)", color=TEXT2)
        ax.tick_params(colors=TEXT2)
        ax.yaxis.grid(True, color=GRID, linewidth=0.8)
        ax.set_axisbelow(True)
        for side in ("top", "right"):
            ax.spines[side].set_visible(False)
        for side in ("left", "bottom"):
            ax.spines[side].set_color(GRID)
        if color_by:
            ax.legend(frameon=False, labelcolor=TEXT, fontsize=9, title=color_by)
        ax.set_title(
            title or ", ".join(self.suites), loc="left", color=TEXT, fontsize=11
        )
        fig.tight_layout()
        return fig

    # ---- dumps --------------------------------------------------------------

    def tables(self) -> dict[str, pd.DataFrame]:
        """Every table, named; the report prints them and ``dump`` writes them."""
        out: dict[str, pd.DataFrame] = {
            "census": self.census(),
            "cells": self.cells(),
            "anomalies": self.anomalies(),
            "round_drift": self.round_drift(),
        }
        for level in LEVELS:
            out[f"position_drift_{level}"] = self.position_drift(level)
        for level in LEVELS:
            out[f"carryover_{level}"] = self.carryover(level)
        return out

    def dump(self, output_dir: Path) -> None:
        """Write every table as TSV, one file per table."""
        output_dir = Path(output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        for name, table in self.tables().items():
            table.to_csv(output_dir / f"{name}.tsv", sep="\t")
