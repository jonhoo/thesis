#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['vote'].query('op == "all" & write_every == 1000 & memlimit == 0 & until > 256 & metric == "sojourn" & partial == True').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['partial'], label="Partial")
data = common.source['redis'].query('op == "all" & write_every == 1000 & until > 256 & metric == "sojourn"').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['redis'], label="Redis")
data = common.source['vote'].query('op == "all" & write_every == 1000 & memlimit == 0 & until > 256 & metric == "sojourn" & partial == False').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['full'], ls='--', label="Full")
ax.xaxis.set_major_formatter(common.kfmt)
ax.set_ylim(0, 100)
ax.set_xlim(0, 15000000)
ax.legend()

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("Latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
