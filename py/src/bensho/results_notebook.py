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
# A results directory (one suite, or every suite under `--out`), through
# `Results`. Tables are dumped to `output_dir` by the pymake task, not here.

# %% tags=["parameters"]
results_path = "../out"
output_dir = "output"

# %%
from pathlib import Path

from bensho.results import Results

results = Results.load([Path(results_path)])

# %% [markdown]
# ## Census
#
# One file per cell. Rows present against the rounds the suite reached, and
# the seeds and round sizes seen, so files left over from an earlier run or a
# partial rerun stand out.

# %%
results.census()

# %% [markdown]
# ## Cells
#
# min / median / CV of ns per op across rounds, keyed by suite, group and
# cell. `calibrated` marks cells whose batch was scaled to the time budget:
# fewer ops behind the rate.

# %%
results.cells()

# %%
results.anomalies()

# %% [markdown]
# ## Drift
#
# Every row normalised to its cell's median. By round: the machine over the
# run. By slot, at three levels: the cell's slot in the flattened round, its
# group's slot among the round's groups, and its own slot among its group's
# cells (a group's state is built fresh per visit, so this last one is what
# earlier cells of the same group left behind).

# %%
results.round_drift()

# %%
results.position_drift("round")

# %%
results.position_drift("group")

# %%
results.position_drift("cell")

# %% [markdown]
# ## Carry-over
#
# Mean normalised ns/op by what ran immediately before, at the same three
# levels. At level `cell`, `(first)` is the cell that ran on the freshly
# built state.

# %%
results.carryover("round")

# %%
results.carryover("group")

# %%
results.carryover("cell")

# %% [markdown]
# ## Figure

# %%
_ = results.figure(color_by="group")
