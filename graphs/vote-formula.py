#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import subprocess
import sys
import io
import os

# first, find smallest supported memlimit at 1M
vote = common.load('vote', only_good = False)
data = vote.query('op == "all" & clients == 4 & write_every == 100')
data = data.query('until == 256 & distribution == "skewed" & metric == "sojourn"')
for load in [1000000, 250000]:
    d = data.query('target == %d' % load)
    fvmrss = d.query('partial == False')['vmrss'].item()
    fopmem = d.query('partial == False')['opmem'].item()

    low = d.query('memlimit != 0')
    low = low.query('achieved >= 0.99 * target & p95 < 20')
    low_mem = low.reset_index()['memlimit'].min()
    print('Using %s memory limit as representative for vote @ %d' % (common.bts(low_mem * 1024 * 1024 * 1024), load))

    # bah -- floating point
    low = low.query('memlimit == %.7f' % low_mem)
    low_opmem = low['opmem'].item()
    perc = 100.0 * low_opmem / fopmem
    print('vote @ %d, limit %s: opmem / full opmem = %.1f%%' % (load, common.bts(low_mem * 1024 * 1024 * 1024), perc))
    print('                     vmrss use = %.1f%%' % (100.0 * low['vmrss'].item() / fvmrss))

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
        ax.plot(d["throughput"], d["percentage"], '.-', color=colors[i], label='%s ($\\alpha$=%.3f)' % (skew, d["alpha"].min()))
        i += 1

ax.plot([250000], [perc], '*', ms=15, color='black')
bbox_props = dict(boxstyle="larrow", fc=common.colors['full'], ec=common.colors['noria'], lw=2)
t = ax.text(480000, 4.8, "Achieved in vote benchmark", ha="center", va="center", rotation=45,
            size=13,
            color=common.colors['noria'],
            bbox=bbox_props)

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_xlabel('Expected number of requests per second')
ax.set_ylabel('Must be cached [\\%]')
ax.set_ylim(0, 10)
ax.legend()

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
