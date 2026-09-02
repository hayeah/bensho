"""bensho-report: print the read-out of one or more bensho CSVs, plot it.

cd py && uv run bensho-report ../toy.csv --plot toy.png
uvx --from ./py bensho-report toy.csv
"""

from __future__ import annotations

from pathlib import Path
from typing import Annotated

import pandas as pd
import typer

from bensho.results import Results

app = typer.Typer(add_completion=False)


@app.command()
def report(
    csv: Annotated[list[Path], typer.Argument(help="bensho CSV files, concatenated")],
    plot: Annotated[Path | None, typer.Option(help="save the figure here")] = None,
    dump: Annotated[
        Path | None, typer.Option(help="write every table as TSV here")
    ] = None,
) -> None:
    """Census, per-cell statistics, anomalies and drift for bensho CSVs."""
    pd.set_option("display.width", 200)
    pd.set_option("display.max_columns", 30)
    r = Results.load(csv)
    sections = [
        ("census", r.census()),
        ("cells", r.cells()),
        ("anomalies", r.anomalies()),
        ("round drift (normalised to cell median)", r.round_drift()),
        ("position drift (slot 0..1 -> normalised ns/op)", r.position_drift()),
        ("carry-over by predecessor", r.carryover()),
    ]
    for name, table in sections:
        print(f"== {name}")
        print(table.to_string() if len(table) else "(none)")
        print()
    if dump is not None:
        r.dump(dump)
        print(f"wrote {dump}/")
    if plot is not None:
        fig = r.figure()
        plot.parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(plot, bbox_inches="tight", facecolor=fig.get_facecolor())
        print(f"wrote {plot}")


def main() -> None:
    app()


if __name__ == "__main__":
    main()
