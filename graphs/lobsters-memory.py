#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

#
# memory use at various lobsters scales
#

prune = common.limited_lobsters
partial_scale = common.lobsters_experiments.query('partial == True').reset_index()['scale'].max()
partial = common.lobsters_experiments.query('partial == True & scale == %d' % (partial_scale))
full_scale = common.lobsters_experiments.query('partial == False').reset_index()['scale'].max()
full = common.lobsters_experiments.query('partial == False & scale == %d' % (full_scale))

fig, (mem, throughput) = plt.subplots(2, 1, sharex = True)

# plot the "bottom" part of the bars
xs = ["Noria w/eviction", "Noria, partial", "Noria, full", "MySQL"]
xticks = [x for x in range(len(xs))]
ys = [
    prune["fopmem"].item(),
    partial["fopmem"].item(),
    full["fopmem"].item(),
    0,
]
bars = mem.bar(xticks, ys, color=common.colors['full'])

# plot the "top" part of the bars
tops = [
    prune["opmem"].item() - prune["fopmem"].item(),
    partial["opmem"].item() - partial["fopmem"].item(),
    full["opmem"].item() - full["fopmem"].item(),
    0
]
bars = mem.bar(xticks, tops, bottom=ys)
bars[0].set_color(common.colors['evict'])
bars[1].set_color(common.colors['partial'])
bars[2].set_color(common.colors['full'])
bars[3].set_color(common.colors['mysql'])

mem.set_xticks(xticks)
mem.set_xticklabels(xs)
mem.set_ylim(0, full["opmem"].max() * 1.3) # also fit labels over bars

mem.set_ylabel("Memory [GB]")

# Attach a text label above each bar with its value.
fmtfn = lambda x : '%1.1fGB' % (x * 1e-9) if x >= 1e9 else '%1.0fMB' % (x * 1e-6) if x >= 1e6 else '%1.10kB' % (x * 1e-3) if x >= 1e3 else '%1.0fB' % x if x > 0 else "No overhead"
for rect in bars:
    height = rect.get_height()
    mem.annotate(fmtfn((rect.get_y() + height) * 1024 * 1024 * 1024),
                xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                xytext=(0, 3),  # 3 points vertical offset
                textcoords="offset points",
                ha='center', va='bottom')

#
# throughput achieved
#

tys = []
for x in xs:
    if x == "Noria w/eviction":
        achieved = prune['achieved'].max()
    elif x == "Noria, partial":
        achieved = partial['achieved'].max()
    elif x == "Noria, full":
        achieved = full['achieved'].max()
    elif x == "MySQL":
        achieved = common.mysql_experiments['achieved'].max()
    tys.append(achieved)

# plot the "bottom" part of the bars
bars = throughput.bar(xticks, tys)
bars[0].set_color(common.colors['evict'])
bars[1].set_color(common.colors['partial'])
bars[2].set_color(common.colors['full'])
bars[3].set_color(common.colors['mysql'])

throughput.set_xticks(xticks)
throughput.set_xticklabels(xs)
throughput.set_ylim(0, common.lobsters_experiments['achieved'].max() * 1.3) # also fit labels over bars
throughput.yaxis.set_major_formatter(common.kfmt)

throughput.set_ylabel("Pages/s")

# Attach a text label above each bar with its value.
fmtfn = lambda x: '%1.1fM' % (x * 1e-6) if x >= 1e6 else '%1.1fK' % (x * 1e-3) if x >= 1e3 else '%1.0f' % x
for rect in bars:
    height = rect.get_height()
    y = fmtfn(rect.get_y() + height)
    throughput.annotate("%s\\textsuperscript{†}" % y if rect.get_x() == 1.6 else y,
                xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                xytext=(0, 3),  # 3 points vertical offset
                textcoords="offset points",
                ha='center', va='bottom')

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
