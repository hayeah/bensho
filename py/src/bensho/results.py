"""The bensho read-out: one class over one DataFrame of harness rows.

The contract with the Rust side is the column list ``BENSHO_COLUMNS`` (see
``specs/20260902_180344_bensho_micro_benchmark_harness.md``, "CSV contract").
Every other column is the bench's own and passes through untouched as
``Results.user_columns``.

Two ideas copied from yudu's ``embed_analysis.py``:

- ``batch`` is per row because the harness calibrates the batch DOWN for
  slow cells to a time budget. ns/op stays comparable (it is a rate), but a
  small batch has fewer ops behind it, so calibrated cells are flagged in
  ``cells()`` and listed in ``anomalies()`` rather than silently averaged in.
- Cells were interleaved inside a round, so round-by-round pairing is
  meaningful: the drift tables normalise every row to its cell's median
  and ask whether the round, the slot, or the predecessor moved it.
"""

from __future__ import annotations

from collections.abc import Iterable
from pathlib import Path

import numpy as np
import pandas as pd

#: The harness's columns, in CSV order.
BENSHO_COLUMNS = [
    "subject",
    "mode",
    "round",
    "position",
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

#: Coefficient of variation above which a cell is listed in ``anomalies()``.
CV_FLAG_PCT = 5.0

#: Categorical palette, fixed first-seen order per subject, never cycled.
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


class Results:
    """Everything the harness's CSV supports, as DataFrames."""

    def __init__(self, df: pd.DataFrame) -> None:
        missing = [c for c in BENSHO_COLUMNS if c not in df.columns]
        if missing:
            raise ValueError(f"not a bensho CSV, missing columns {missing}")
        if df.empty:
            raise EmptyDataFrameError("no rows")
        if "source" not in df.columns:
            df = df.assign(source="")
        self.df = df.reset_index(drop=True)

    @classmethod
    def load(cls, paths: Iterable[Path | str]) -> Results:
        """Concatenate any number of bensho CSVs; ``source`` names the file."""
        frames = []
        for p in paths:
            p = Path(p)
            frames.append(pd.read_csv(p).assign(source=p.name))
        if not frames:
            raise EmptyDataFrameError("no CSV paths")
        return cls(pd.concat(frames, ignore_index=True))

    # ---- shape --------------------------------------------------------------

    @property
    def user_columns(self) -> list[str]:
        """The bench's own columns, in CSV order."""
        return [c for c in self.df.columns if c not in BENSHO_COLUMNS and c != "source"]

    @property
    def subjects(self) -> list[str]:
        """Subjects in first-seen order; the colour order of every plot."""
        return list(dict.fromkeys(self.df["subject"]))

    @property
    def modes(self) -> list[str]:
        return list(dict.fromkeys(self.df["mode"]))

    def census(self) -> pd.DataFrame:
        """Per source: rows present against ``cells x rounds`` expected."""
        rows = []
        for source, d in self.df.groupby("source", sort=False):
            cells = int(d["cells"].max())
            rounds = int(d["round"].max())
            rows.append(
                {
                    "source": source,
                    "rows": len(d),
                    "cells": cells,
                    "rounds": rounds,
                    "expected": cells * rounds,
                    "missing": cells * rounds - len(d),
                    "seed": int(d["seed"].iloc[0]),
                    "user_columns": ",".join(self.user_columns),
                }
            )
        return pd.DataFrame(rows).set_index("source")

    # ---- per cell -----------------------------------------------------------

    def cells(self) -> pd.DataFrame:
        """Per (subject, mode): round statistics of ns/op, batch and verdict."""
        g = self.df.groupby(["subject", "mode"], sort=False)
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
            subject, mode = row["subject"], row["mode"]
            why = []
            if row["calibrated"]:
                why.append(f"batch {row['calibration']} to {int(row['batch']):,}")
            if pd.notna(row["cv_pct"]) and row["cv_pct"] > CV_FLAG_PCT:
                why.append(f"CV {row['cv_pct']:.1f}%")
            if why:
                flags.append(
                    {
                        "subject": subject,
                        "mode": mode,
                        "batch": int(row["batch"]),
                        "cv_pct": row["cv_pct"],
                        "note": "; ".join(why),
                    }
                )
        return pd.DataFrame(
            flags, columns=["subject", "mode", "batch", "cv_pct", "note"]
        )

    # ---- drift, from the recorded positions ---------------------------------

    def normalized(self) -> pd.DataFrame:
        """The rows plus ``norm``: ns/op over the cell's median, and ``slot``:
        position as a fraction of the round (0 first, 1 last)."""
        d = self.df.copy()
        med = d.groupby(["source", "subject", "mode"])["ns_per_op"].transform("median")
        d["norm"] = d["ns_per_op"] / med
        span = (d["cells"] - 1).clip(lower=1)
        d["slot"] = d["position"] / span
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

    def position_drift(self) -> pd.DataFrame:
        """Does the slot a cell drew move its number?

        ``slope_pct`` is the fitted change in normalised ns/op from the first
        slot to the last, in percent; ``corr`` the Pearson correlation of
        ``norm`` with ``slot``. One row for all cells, then one per subject.
        """
        d = self.normalized()
        rows = [self._slot_fit("(all)", d)]
        for s in self.subjects:
            rows.append(self._slot_fit(s, d[d["subject"] == s]))
        return pd.DataFrame(rows).set_index("subject")

    @staticmethod
    def _slot_fit(name: str, d: pd.DataFrame) -> dict[str, object]:
        x = d["slot"].to_numpy(float)
        y = d["norm"].to_numpy(float)
        if len(d) < 3 or np.ptp(x) == 0:
            return {
                "subject": name,
                "rows": len(d),
                "slope_pct": np.nan,
                "corr": np.nan,
            }
        slope, _ = np.polyfit(x, y, 1)
        corr = float(np.corrcoef(x, y)[0, 1]) if np.std(y) > 0 else np.nan
        return {
            "subject": name,
            "rows": len(d),
            "slope_pct": round(100 * float(slope), 2),
            "corr": round(corr, 3) if pd.notna(corr) else np.nan,
        }

    def carryover(self) -> pd.DataFrame:
        """Mean normalised ns/op of each subject, grouped by the subject that
        ran immediately before it in the round (``(first)`` for slot 0).

        The reason the harness shuffles: a slow predecessor that consistently
        inflates its successor shows up here as a row above 1."""
        d = self.normalized()
        key = ["source", "round", "position"]
        prev = d[key + ["subject"]].copy()
        prev["position"] = prev["position"] + 1
        prev = prev.rename(columns={"subject": "predecessor"})
        merged = d.merge(prev, on=key, how="left")
        merged["predecessor"] = merged["predecessor"].fillna("(first)")
        out = merged.groupby(["subject", "predecessor"], sort=False).agg(
            rows=("norm", "size"), mean_norm=("norm", "mean")
        )
        return out.round(4)

    # ---- figure -------------------------------------------------------------

    def figure(self, title: str | None = None):
        """Median ns/op per cell with min-max whiskers: modes along x, one
        colour per subject in first-seen order, log y."""
        import matplotlib.pyplot as plt

        cells = self.cells().reset_index()
        subjects, modes = self.subjects, self.modes
        if len(subjects) > len(PALETTE):
            raise ValueError(f"more than {len(PALETTE)} subjects; facet instead")
        colour = dict(zip(subjects, PALETTE, strict=False))
        width = 0.8 / max(len(subjects), 1)

        fig, ax = plt.subplots(figsize=(max(6, 1.6 * len(modes) + 3), 5), dpi=150)
        fig.patch.set_facecolor(SURFACE)
        ax.set_facecolor(SURFACE)
        for j, s in enumerate(subjects):
            d = cells[cells["subject"] == s].set_index("mode")
            xs, meds, lo, hi = [], [], [], []
            for i, m in enumerate(modes):
                if m not in d.index:
                    continue
                r = d.loc[m]
                xs.append(i - 0.4 + width * (j + 0.5))
                meds.append(r["median"])
                lo.append(r["median"] - r["min"])
                hi.append(r["max"] - r["median"])
            if not xs:
                continue
            ax.errorbar(
                xs,
                meds,
                yerr=[lo, hi],
                fmt="o",
                color=colour[s],
                ecolor=colour[s],
                markersize=7,
                markeredgecolor=SURFACE,
                markeredgewidth=1.4,
                elinewidth=1.4,
                capsize=3,
                label=s,
                zorder=3,
            )
        ax.set_yscale("log")
        ax.set_xticks(range(len(modes)))
        ax.set_xticklabels(modes, color=TEXT2)
        ax.set_ylabel("ns per op (median; whiskers min to max)", color=TEXT2)
        ax.tick_params(colors=TEXT2)
        ax.yaxis.grid(True, color=GRID, linewidth=0.8)
        ax.set_axisbelow(True)
        for side in ("top", "right"):
            ax.spines[side].set_visible(False)
        for side in ("left", "bottom"):
            ax.spines[side].set_color(GRID)
        ax.legend(frameon=False, labelcolor=TEXT, fontsize=9)
        sources = ", ".join(dict.fromkeys(self.df["source"])) or "bensho"
        ax.set_title(title or sources, loc="left", color=TEXT, fontsize=11)
        fig.tight_layout()
        return fig

    # ---- dumps --------------------------------------------------------------

    def dump(self, output_dir: Path) -> None:
        """Write every table as TSV, one file per method."""
        output_dir = Path(output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        for name in (
            "census",
            "cells",
            "anomalies",
            "round_drift",
            "position_drift",
            "carryover",
        ):
            getattr(self, name)().to_csv(output_dir / f"{name}.tsv", sep="\t")
