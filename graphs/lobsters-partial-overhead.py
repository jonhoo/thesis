#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

fig, ax = plt.subplots()
data = common.source['lobsters-noria'].query('op == "all" & memlimit == 0 & until > 256 & metric == "sojourn" & partial == True').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['partial'], label="Partial")
data = common.source['lobsters-noria'].query('op == "all" & memlimit == 0 & until > 256 & metric == "sojourn" & partial == False').sort_index().reset_index()
ax.plot(data["achieved"], data["p90"], '.-', color=common.colors['full'], label="Full")
ax.set_ylim(0, 100)
ax.legend()

ax.set_xlabel("Achieved throughput [pages per second]")
ax.set_ylabel("Latency [ms]")

plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf", bbox_inches="tight", pad=0.001)
