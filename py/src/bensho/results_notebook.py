# ---
# jupyter:
#   jupytext:
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#   kernelspec:
#     display_name: Python 3
#     language: python
#     name: python3
# ---

# %% [markdown]
# # bensho read-out
#
# One harness CSV (or several concatenated), through `Results`. Tables are
# dumped to `output_dir` by the pymake task, not here.

# %% tags=["parameters"]
csv_path = "../toy.csv"
output_dir = "output"

# %%
from pathlib import Path

from bensho.results import Results

results = Results.load([Path(csv_path)])

# %% [markdown]
# ## Census
#
# Rows present against `cells x rounds` expected, per source file.

# %%
results.census()

# %% [markdown]
# ## Cells
#
# min / median / CV of ns per op across rounds. `calibrated` marks cells whose
# batch was scaled to the time budget: fewer ops behind the rate.

# %%
results.cells()

# %%
results.anomalies()

# %% [markdown]
# ## Drift
#
# Every row normalised to its cell's median. By round: the machine over the
# run. By slot: whether the position a cell drew matters. By predecessor:
# whether a slow cell inflates whatever ran after it.

# %%
results.round_drift()

# %%
results.position_drift()

# %%
results.carryover()

# %% [markdown]
# ## Figure

# %%
_ = results.figure()
