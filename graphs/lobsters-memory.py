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
base["vmrss"] = base["vmrss"] / (1024 * 1024 * 1024)
prune_limit = base.query('memlimit != 0 & durable == False').reset_index()['memlimit'].min()
print('Using %s memory limit as representative for lobsters' % (common.bts(prune_limit * 1024 * 1024 * 1024)))
prune_limit_dur = base.query('memlimit != 0 & durable == True').reset_index()['memlimit'].min()
print('Using %s memory limit as representative for durable lobsters' % (common.bts(prune_limit_dur * 1024 * 1024 * 1024)))
prune = base.query('durable == False & memlimit == %f' % (prune_limit))
prune_dur = base.query('durable == True & memlimit == %f' % (prune_limit_dur))
partial = base.query('partial == True & memlimit == 0')
full = base.query('partial == False & memlimit == 0')

fig, mem = plt.subplots()

xs = [
    "Noria",
    "Noria without partial",
]
xticks = [x for x in range(len(xs))]
width = 0.35

#
# memory used
#

print('Partial non-durable compared to full: %.1f%%' % (100.0 * float(prune_dur["vmrss"].item()) / full.query('durable == True')["vmrss"].item()))
bars1 = mem.bar([x - width/2 for x in xticks], [prune['vmrss'].item(), full.query('durable == False')['vmrss'].item()], width, label = "In-memory base tables")
bars1[0].set_color(common.colors['noria'])
bars1[1].set_color(common.colors['noria'])

bars2 = mem.bar([x + width/2 for x in xticks], [prune_dur['vmrss'].item(), full.query('durable == True')['vmrss'].item()], width, label = "Durable base tables")
bars2[0].set_color(common.colors['full'])
bars2[1].set_color(common.colors['full'])

mem.set_xticks(xticks)
mem.set_xticklabels(xs)
mem.set_ylim(0, 128 * 1.1) # also fit labels over bars
mem.set_yticks([0, 32, 64, 96, 128])
mem.legend(loc = 'upper left')

mem.set_ylabel("Resident virtual memory [GB]")

# Attach a text label above each bar with its value.
for rect in bars1 + bars2:
    height = rect.get_height()
    mem.annotate(bfmtfn((rect.get_y() + height) * 1024 * 1024 * 1024),
                xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                xytext=(0, 3),  # 3 points vertical offset
                textcoords="offset points",
                ha='center', va='bottom')

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
