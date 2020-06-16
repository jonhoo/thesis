#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & clients == 6 & write_every == 20 & until > 256 & metric == "sojourn" & partial == True').sort_index().reset_index()
limits = data.groupby('memlimit').tail(1)
limits = [l for l in limits["memlimit"]]
limits.sort(reverse=True)
i = 0
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    if limit == 0:
        ax.plot(d["achieved"], d["p90"], '-.', color = 'black', lw=1, label = "unlimited")
    else:
        ax.plot(d["achieved"], d["p90"], '-.', color = common.memlimit_colors[i], lw=1, alpha = 0.8, label = common.bts(limit * 1024 * 1024 * 1024))
        i += 1

ax.set_ylim(0, 100)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("Latency [ms]")

plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf", bbox_inches="tight", pad=0.001)
