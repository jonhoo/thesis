#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & clients == 6 & write_every == 20 & until > 256 & distribution == "skewed" & metric == "sojourn"').sort_index().reset_index()
limits = data.groupby('memlimit').tail(1)
limits = [l for l in limits["memlimit"]]
limits.sort(reverse=True)
print(limits)
limits = [0, 256 / 1024.0,  384 / 1024.0, 448 / 1024.0, 512 / 1024.0, 576 / 1024.0, 768 / 1024.0]
limits.sort(reverse=True)
i = 0
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    if limit == 0:
        dd = d.query("partial == True")
        ax.plot(dd["achieved"], dd["median"], '.-', color = 'black', lw=1, alpha = 0.8, label = "unlimited")
        # dd = d.query("partial == False")
        # ax.plot(dd["achieved"], dd["median"], '.--', color = 'black', lw=1, alpha = 0.8, label = "full")
    else:
        ax.plot(d["achieved"], d["median"], '.-', color = common.memlimit_colors[i], lw=1, alpha = 0.8, label = common.bts(limit * 1024 * 1024 * 1024))
        i += 1

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_ylim(0, 100)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("Latency [ms]")

plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf", bbox_inches="tight", pad=0.001)
