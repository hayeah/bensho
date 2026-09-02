"""Makefile.py for the bensho analysis package.

pymake all                                  # lint, typecheck, test
BENSHO_RESULTS=../out pymake notebook       # read-out notebook into output/
"""

import os
from pathlib import Path

from pymake import sh, task

SRC = Path("src/bensho")
OUTPUT_DIR = Path("output")
NOTEBOOK_SRC = SRC / "results_notebook.py"
NOTEBOOK_IPYNB = OUTPUT_DIR / "results_notebook.ipynb"
NOTEBOOK_DUMP_DIR = OUTPUT_DIR / "results_notebook.dump"


@task()
def lint():
    """Run ruff."""
    sh(f"uv run ruff check {SRC}")
    sh(f"uv run ruff format --check {SRC}")


@task()
def typecheck():
    """Run pyright."""
    sh("uv run pyright")


@task()
def format():
    """Format with ruff."""
    sh(f"uv run ruff format {SRC}")
    sh(f"uv run ruff check --fix {SRC}", check=False)


@task()
def test():
    """Run pytest."""
    sh(f"uv run pytest -q {SRC}")


@task(inputs=[lint, typecheck, test])
def all():
    """Lint, typecheck, test."""


@task()
def notebook():
    """Convert and execute the read-out notebook against $BENSHO_RESULTS."""
    results = os.environ.get("BENSHO_RESULTS", "../out")
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    sh(f"uv run jupytext --to notebook -o {NOTEBOOK_IPYNB} {NOTEBOOK_SRC}")
    sh(
        f"uv run papermill {NOTEBOOK_IPYNB} {NOTEBOOK_IPYNB} "
        f"-p results_path {results} -p output_dir {NOTEBOOK_DUMP_DIR}"
    )
    sh(f"uv run bensho-report {results} --dump {NOTEBOOK_DUMP_DIR}")


task.default(all)
