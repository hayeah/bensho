"""bensho-report: print the read-out of bensho CSV files or directories, plot it.

cd py && uv run bensho-report ../out/toy --plot toy.png --color-by group
uvx --from ./py bensho-report out/          # every suite under out/
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
    paths: Annotated[
        list[Path], typer.Argument(help="bensho CSV files or directories of them")
    ],
    plot: Annotated[Path | None, typer.Option(help="save the figure here")] = None,
    dump: Annotated[
        Path | None, typer.Option(help="write every table as TSV here")
    ] = None,
    color_by: Annotated[
        str | None, typer.Option(help="colour the figure by this column")
    ] = None,
) -> None:
    """Census, per-cell statistics, anomalies, drift and carry-over at every
    level for bensho results."""
    pd.set_option("display.width", 200)
    pd.set_option("display.max_columns", 30)
    r = Results.load(paths)
    for name, table in r.tables().items():
        print(f"== {name}")
        print(table.to_string() if len(table) else "(none)")
        print()
    if dump is not None:
        r.dump(dump)
        print(f"wrote {dump}/")
    if plot is not None:
        fig = r.figure(color_by=color_by)
        plot.parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(plot, bbox_inches="tight", facecolor=fig.get_facecolor())
        print(f"wrote {plot}")


def main() -> None:
    app()


if __name__ == "__main__":
    main()
