---
name: python
description: Coding convention, style guide, and tooling for writing Python.
---

# Python Project

Project setup

- use uv with pyproject.toml
- define both project and project.scripts
- proper src structure for internal package imports
- .gitignore: https://raw.githubusercontent.com/github/gitignore/master/Python.gitignore

Code quality
- ruff for autoformat
- pyright for type checking
- pytest for unit tests
	- put test files beside the source file
	- foo.py should have foo_test.py

CLI tools

- use typer for the cli main script
- install with `uv tool install -e .`
- for complex output, prefer output in JSON.

## Python coding style

* Code structure
	* When writing a fairly complex feature, avoid having a "bag of loose functions" that call each other and pass parameters around.
	* Group related methods together in a class.
	* Group common data as properties of a class instead of using parameters to pass state around.
	* Use type annotations and dataclasses.
	* Use a simple dependency injection style similar to Go's "wire" system.
	  * Avoid deep, recursive initializations.
	  * Prefer factory functions or class constructors that build an acyclic dependency graph to produce the final object.

* Error handling and cleanup
	* Avoid `try ... except` unless you have good reasons.
	* It is fine to fail fast: let the error raise and the app crash, rather than adding a `try ... except` just to wrap or re-report it.
	* Prefer using `with` statements to handle cleanup.
	* Use `try ... except` if:
	  * You have a clear recovery strategy.
	  * The code runs in a loop and you want to keep going after individual failures.

* Loops and helper functions
	* If you are writing a loop and the loop body is complicated, extract a helper function or method that encapsulates the processing for a single item.

* Naming and file layout
    * Function naming:
      * Avoid the `get_` prefix for data-oriented functions.
      * Prefer a bare `noun` (preferred), or `imperativeverb_noun`.
    * File naming conventions:
      * Put test files beside the source file.
      * `foo.py` should have `foo_test.py`.
      * If asked to write Markdown documentation for `foo.py`, write it in `foo.md`.

  * Testing (pytest)
    * For tests that have varied and clear input/output expectations, use data-driven tests to reduce code redundancy.

* Dataclasses and JSON serialization
  * For dataclasses that require JSON serialization, define and use this mixin:

```python
class JsonMixin:
    """Mixin to add JSON serialization to dataclasses."""

    def to_dict(self) -> dict[str, Any]:
        """
        Return a JSON-serializable dict representation.

        For dataclasses, this uses dataclasses.asdict, which also
        recursively converts nested dataclasses.
        """
        if is_dataclass(self):
            return asdict(self)
        # Fallback if you ever use this on a non-dataclass
        return self.__dict__

    def to_json(self, **json_kwargs: Any) -> str:
        """
        Return a JSON string representation.

        Extra keyword args are passed through to json.dumps
        (e.g. indent=2, sort_keys=True).
        """
        return json.dumps(self.to_dict(), **json_kwargs)
```

## HTTP Client

If an efficient HTTP client is called for (e.g. downloading many files), prefer `httpx[http2]` — async HTTP client with HTTP/2 multiplexing.

```
uv add "httpx[http2]"
```

## Terminal Output

Use `rich` if rich terminal output is called for:

https://github.com/Textualize/rich

## Makefile.py

See [pymake](./pymake.md) for a detailed example of creating a Makefile.py for project build tasks. You MUST read this documentation when you first create Makefile.py.

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
```

## Python Notebook

If you are working on a Jupyter notebook, you MUST read [./notebook.md](./notebook.md).

## Logging

Use `hayeah.core.logger` for structured logging. See `libs/hayeah-py/src/hayeah/core/logger/` for the full spec.

```py
from hayeah.core.logger import new

log = new("my-tool")

log.info("message_sent", channel="telegram", duration_ms=120)
log.error("api_failed", status=429, retry_after=30)
```

- `new(name)` returns a structlog `BoundLogger`. Same name returns the same logger (idempotent, safe to call from multiple modules).
- `LOG_LEVEL` env var overrides log level (default: `INFO`).
- Console output is colored and human-readable. File output is JSONL (5 MB rotation, 3 backups).
- View logs with `lnav ~/.local/log/` or `tail -f ~/.local/log/*.jsonl | jq .`

### Adding to a tool's dependencies

Add `hayeah-core` as an editable dependency in the tool's `pyproject.toml`:

```toml
[project]
dependencies = [
    "hayeah-core",
    # ... other deps
]

[tool.uv.sources]
hayeah-core = { path = "../../libs/hayeah-py", editable = true }
```

Adjust the relative path as needed for the tool's location relative to `libs/hayeah-py/`.

### Other shared modules

See `libs/README.md` for the full index of cross-language convention libraries:
- `hayeah.core.fzfmatch` — fuzzy path matcher
- `hayeah.core.config` — single-envar config loading
- `hayeah.core.shortid` — short ID generation and prefix resolution

