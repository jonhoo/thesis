#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

# compute subset of data for memory-limited lobsters
base = common.load('lobsters')
limited_lobsters_scale = base.query('memlimit != 0').reset_index()['scale'].max()
limited_lobsters = base.query('memlimit != 0 & scale == %d' % limited_lobsters_scale).groupby('memlimit').tail(1).reset_index()
limited_lobsters_still_ok = limited_lobsters.query('achieved >= 0.99 * requested & mean < 50')["memlimit"].min()
limited_lobsters = base.query('memlimit == %f & scale == %d' % (limited_lobsters_still_ok, limited_lobsters_scale)).tail(1).copy()
print('Using %.0fMB memory limit as representative for lobsters (%d pages/s)' % (limited_lobsters_still_ok * 1024, limited_lobsters["achieved"].min()))

prune = limited_lobsters
partial_scale = base.query('partial == True').reset_index()['scale'].max()
partial = base.query('partial == True & scale == %d' % (partial_scale))
full_scale = base.query('partial == False').reset_index()['scale'].max()
full = base.query('partial == False & scale == %d' % (full_scale))

fig, throughput = plt.subplots()

xs = [
    "Noria",
    "Noria without partial",
    "MySQL"
]
xticks = [x for x in range(len(xs))]

#
# throughput achieved
#

tys = []
for x in xs:
    if x == "Noria":
        achieved = prune['achieved'].max()
    elif x == "Noria without partial":
        achieved = full['achieved'].max()
    elif x == "MySQL":
        achieved = common.load('lobsters-mysql')['achieved'].max()
    tys.append(achieved)

bars = throughput.bar(xticks, tys)
bars[0].set_color(common.colors['noria'])
bars[1].set_color(common.colors['full'])
bars[2].set_color(common.colors['mysql'])

throughput.set_xticks(xticks)
throughput.set_xticklabels(xs)
throughput.set_ylim(0, max(tys) * 1.15) # also fit labels over bars
throughput.yaxis.set_major_formatter(common.kfmt)

throughput.set_ylabel("Pages/s")

# Attach a text label above each bar with its value.
fmtfn = lambda x: '%1.1fM' % (x * 1e-6) if x >= 1e6 else '%1.1fk' % (x * 1e-3) if x >= 1e3 else '%1.0f' % x
for rect in bars:
    height = rect.get_height()
    y = fmtfn(rect.get_y() + height)
    throughput.annotate(y,
                xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                xytext=(0, 3),  # 3 points vertical offset
                textcoords="offset points",
                ha='center', va='bottom')

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
