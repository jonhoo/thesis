#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & clients == 4 & write_every == 100 & until == 128 & distribution == "skewed" & metric == "sojourn"').sort_index().reset_index()
limits = data.groupby('memlimit').tail(1)
limits = [l for l in limits["memlimit"]]
limits.sort()
print(limits)
limits = [256 / 1024.0,  320 / 1024.0, 448 / 1024.0]
colors = common.memlimit_colors(len(limits))
limits.sort()
limits = limits + [0]
i = 0
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    if limit == 0:
        dd = d.query("partial == True")
        # we need to make sure we measure the memory use
        # at the same throughput level for all the lines.
        opmem = dd.query('target == 2000000')["vmrss"].item()
        ax.plot(dd["achieved"], dd["mean"], '.-', lw=0.7, color = 'black', label = "%s (no eviction)" % common.bts(opmem))
    else:
        opmem = d.query('target == 2000000')["vmrss"].item()
        ax.plot(d["achieved"], d["mean"], '.-', lw=0.7, color = colors[i], label = '%s' % (common.bts(opmem)))
        i += 1

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_ylim(0, 20000)
# leave some space for legend:
# ax.set_xlim(0, 8000000 * 1.2)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("Mean latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
