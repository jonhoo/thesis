#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & join == False & write_every == 1000 & memlimit == 0 & until == 256 & metric == "sojourn" & partial == True').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['partial'], label="Noria")
# data = common.source['vote'].query('op == "all" & join == False & write_every == 1000 & memlimit == 0 & until == 256 & metric == "sojourn" & partial == False').sort_index().reset_index()
# ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['full'], ls='--', label="Noria, full state")
data = common.source['redis'].query('op == "all" & write_every == 1000 & until == 256 & metric == "sojourn"').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-.', color=common.colors['redis'], label="Redis")
ax.xaxis.set_major_formatter(common.kfmt)
rmx = data.query("achieved >= 0.99 * target & p90 < 10")["achieved"].max()
print(rmx, 16 * rmx)
rmx = 16 * rmx
ax.axvline(rmx, ls='-.', color=common.colors['redis'])
data = common.source['hybrid'].query('op == "all" & write_every == 1000 & until == 256 & metric == "sojourn"').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.--', color=common.colors['mysql'], label="MySQL + Redis")
mmx = data.query("achieved >= 0.99 * target & p90 < 10")["achieved"].max()
print(mmx, 16 * mmx)
mmx = 16 * mmx
ax.axvline(mmx, ls='--', color=common.colors['mysql'])
# ax.set_xlim(0, 30000000)
ax.set_ylim(0, 20)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("90th \\%-ile latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
