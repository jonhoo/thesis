#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import subprocess
import sys
import io
import os

with open(os.path.join(os.path.dirname(__file__), '..', 'benchmarks', 'results', 'vote-formula', 'results.log'), 'r') as f:
    data = pd.read_table(f)

fig, ax = plt.subplots()
data = data.sort_values(by = ["alpha", "throughput"], ascending=[False, True])
skews = data.groupby("skew").tail(1)["skew"]
colors = common.memlimit_colors(len(skews))
i = 0
for skew in skews:
    d = data.query("skew == '%s'" % skew)
    if skew == "uniform":
        ax.plot(d["throughput"], d["percentage"], '.-.', color='black', label='uniform')
    else:
        ax.plot(d["throughput"], d["percentage"], '.-', color=colors[i], label='%s ($\\alpha = %.3f$)' % (skew, d["alpha"].min()))
        i += 1

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_xlabel('Expected number of requests per second')
ax.set_ylabel('Must be cached [\\%]')
ax.set_ylim(0, 50)
ax.legend()

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
