#!/usr/bin/env python3

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

scale = 6000

#
# memory use at various lobsters scales
#
bfmtfn = lambda x : '%1.1fGB' % (x * 1e-9) if x >= 1e9 else '%1.0fMB' % (x * 1e-6) if x >= 1e6 else '%1.10kB' % (x * 1e-3) if x >= 1e3 else '%1.0fB' % x if x > 0 else "No overhead"

base = common.load('lobsters').query('until == 256 & scale == %d & metric == "sojourn"' % (scale))
base["opmem"] = base["opmem"] / (1024 * 1024 * 1024)
prune_limit = base.query('memlimit != 0 & durable == False').reset_index()['memlimit'].min()
print('Using %s memory limit as representative for lobsters' % (common.bts(prune_limit * 1024 * 1024 * 1024)))
prune = base.query('durable == False & memlimit == %f' % (prune_limit))
full = base.query('partial == False & memlimit == 0')

fig, mem = plt.subplots()

xs = [
    "Noria",
    "Noria without partial",
]
xticks = [x for x in range(len(xs))]

#
# memory used
#

bars = mem.bar(xticks, [prune['opmem'].item(), full.query('durable == False')['opmem'].item()])
bars[0].set_color(common.colors['noria'])
bars[1].set_color(common.colors['full'])

mem.set_xticks(xticks)
mem.set_xticklabels(xs)
mem.set_ylim(0, 16 * 1.1) # also fit labels over bars
mem.set_yticks([0])

mem.set_ylabel("Operator data size")

# Attach a text label above each bar with its value.
for rect in bars:
    height = rect.get_height()
    mem.annotate(bfmtfn((rect.get_y() + height) * 1024 * 1024 * 1024),
                xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                xytext=(0, 3),  # 3 points vertical offset
                textcoords="offset points",
                ha='center', va='bottom')

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
# for thesis presentation backup slides:
# plt.savefig("{}.png".format(sys.argv[1]), format="png", dpi=256)
