#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & join == False & write_every == 1000 & memlimit == 0 & until == 256 & metric == "sojourn" & partial == True').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', lw=0.8, color=common.colors['partial'], label="Noria, partial state")
data = common.source['vote'].query('op == "all" & join == False & write_every == 1000 & memlimit == 0 & until == 256 & metric == "sojourn" & partial == False').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', lw=0.8, color=common.colors['full'], ls='--', label="Noria, full state")
data = common.source['hybrid'].query('op == "all" & write_every == 1000 & until == 256 & metric == "sojourn"').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.--', lw=0.8, color=common.colors['mysql'], label="MySQL + Redis")
data = common.source['redis'].query('op == "all" & write_every == 1000 & until == 256 & metric == "sojourn"').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-.', lw=0.8, color=common.colors['redis'], label="Redis, 1 core")
ax.xaxis.set_major_formatter(common.kfmt)
mx = data.query("achieved >= 0.99 * target & p90 < 100")["achieved"].max()
print(mx, 16 * mx)
mx = 16 * mx
# ax.axvline(mx, ls='--', color=common.colors['redis'], label="Redis, 16 cores (extrapolated)")
ax.set_xlim(0, 15000000)
ax.set_ylim(0, 50)
# ax.set_xlim(0, 15000000)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("Latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
