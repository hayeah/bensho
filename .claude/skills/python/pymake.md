
## Makefile.py for project tasks

Use [hayeah-pymake](https://github.com/hayeah/pymake) for project tasks.

Here's a drop-in `Makefile.py` to help you get started (tweak it for your own project):

```py
"""Makefile.py for pymake project."""

from pymake import sh, task

@task()
def lint():
    """Run ruff linter."""
    sh("ruff check src/pymake")


@task()
def typecheck():
    """Run mypy type checker."""
    sh("mypy src/pymake")

@task()
def format():
    """Format code with ruff."""
    sh("ruff format src/pymake")
    sh("ruff check --fix src/pymake", check=False)

@task()
def test():
    """Run pytest."""
    sh("pytest -v src/pymake")

@task(inputs=[lint, typecheck, format, test])
def all():
    pass


@task()
def build():
    """Build package with uv."""
    sh("rm -f dist/*.whl dist/*.tar.gz")
    sh("uv build")


@task(inputs=[build])
def publish():
    """Publish package to PyPI."""
    sh('UV_PUBLISH_TOKEN="op://Personal/PyPI/api publish token" op run -- uv publish')


task.default(all)
```

To install

```
# Run directly without installing
uvx --from hayeah-pymake pymake --help

# Or install globally
uv tool install hayeah-pymake
pymake --help
```

A more involved example:

```py
"""Data processing pipeline with pymake.

Run with: pymake
List tasks: pymake list
"""

from pathlib import Path

from pymake import sh, task

# Configuration
OUTPUT_DIR = Path("output")
DATA_DIR = Path("data")

# Output files
RAW_DATA = OUTPUT_DIR / "raw.json"
PROCESSED = OUTPUT_DIR / "processed.json"
STATS = OUTPUT_DIR / "stats.json"
REPORT = OUTPUT_DIR / "report.html"
DATABASE = OUTPUT_DIR / "data.db"


# Task with outputs only: runs if output is missing
@task(outputs=[RAW_DATA])
def fetch():
    """Download raw data from API."""
    sh(f"curl -o {RAW_DATA} https://api.example.com/data")


# Multiple outputs: both files are produced together
@task(inputs=[RAW_DATA], outputs=[PROCESSED, STATS])
def process():
    """Transform raw data and compute statistics."""
    sh(f"python scripts/transform.py {RAW_DATA} {PROCESSED} {STATS}")


# Depend on one output: still runs process, which produces both PROCESSED and STATS
@task(inputs=[PROCESSED], outputs=[DATABASE])
def load_db():
    """Load processed data into SQLite database."""
    sh(f"python scripts/load_db.py {PROCESSED} {DATABASE}")


# Mix file and task inputs: STATS is a file, load_db is a task
@task(inputs=[STATS, load_db], outputs=[REPORT])
def report():
    """Generate HTML report with statistics."""
    sh(f"python scripts/report.py {DATABASE} {STATS} {REPORT}")


# Meta task: no body, just ensures dependencies run
@task(inputs=[report])
def pipeline():
    """Run full pipeline: fetch → process → load → report."""
    pass


# Phony task: no outputs, so it always runs when invoked
@task()
def lint():
    """Run code linting."""
    sh("ruff check scripts/")


@task()
def test():
    """Run tests."""
    sh("pytest tests/")


@task(inputs=[lint, test])
def check():
    """Run all checks (lint + test)."""
    pass


@task()
def clean():
    """Remove all generated files."""
    sh(f"rm -rf {OUTPUT_DIR}")


# Default task: runs when pymake is invoked without arguments
task.default(pipeline)
```