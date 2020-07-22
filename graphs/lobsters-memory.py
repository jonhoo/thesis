#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

#
# memory use at various lobsters scales
#
fmtfn = lambda x : '%1.1fGB' % (x * 1e-9) if x >= 1e9 else '%1.0fMB' % (x * 1e-6) if x >= 1e6 else '%1.10kB' % (x * 1e-3) if x >= 1e3 else '%1.0fB' % x if x > 0 else "No overhead"

# compute subset of data for memory-limited lobsters
limited_lobsters_scale = common.lobsters.query('op == "all" & memlimit != 0 & achieved >= 0.99 * requested & mean < 50').reset_index()['scale'].max()
limited_lobsters = common.lobsters.query('op == "all" & memlimit != 0 & scale == %d' % limited_lobsters_scale).groupby('memlimit').tail(1).reset_index()
limited_lobsters_still_ok = limited_lobsters.query('achieved >= 0.99 * requested & median < 50')["memlimit"].min()
limited_lobsters = common.lobsters.query('op == "all" & memlimit == %f & scale == %d' % (limited_lobsters_still_ok, limited_lobsters_scale)).tail(1).copy()
print('Using %.0fMB memory limit as representative for lobsters (%d pages/s)' % (limited_lobsters_still_ok * 1024, limited_lobsters["achieved"].min()))

prune = limited_lobsters
partial_scale = common.lobsters_experiments.query('partial == True').reset_index()['scale'].max()
partial = common.lobsters_experiments.query('partial == True & scale == %d' % (partial_scale))
full_scale = common.lobsters_experiments.query('partial == False').reset_index()['scale'].max()
full = common.lobsters_experiments.query('partial == False & scale == %d' % (full_scale))

# find closest scale for prune, and show vmrss
candidates = common.lobsters.query('op == "all" & memlimit != 0 & achieved >= 0.99 * requested & mean < 50')
candidate = limited_lobsters_scale
for scale in candidates.reset_index()['scale']:
    if abs(scale - full_scale) < abs(candidate - full_scale):
        candidate = scale
mem_at_candidate = candidates.query('scale == %d' % candidate)['vmrss'].min()
nxt = candidates.query('scale > %d' % candidate).reset_index()['scale'].min()
mem_at_nxt = candidates.query('scale == %d' % nxt)['vmrss'].min()
# linear iterpolation
a = (mem_at_nxt - mem_at_candidate) / (nxt - candidate)
b = mem_at_candidate - a * candidate
mem_at_full_apx = a * full_scale + b
print("VmRSS for prune @ full-ish (%d-%d vs %d): %s" % (candidate, nxt, full_scale, fmtfn(mem_at_full_apx * 1024 * 1024 * 1024)))

fig, throughput = plt.subplots(1, 1)

xs = [
    "Noria",
    # "Noria, partial",
    "Noria, no partial",
    "MySQL"
]
xticks = [x for x in range(len(xs))]

print("%s base mem: %s" % (xs[0], fmtfn(prune['basemem'].item() * 1024 * 1024 * 1024)))
print("%s base mem: %s" % ("partial", fmtfn(partial['basemem'].item() * 1024 * 1024 * 1024)))
print("%s base mem: %s" % (xs[1], fmtfn(full['basemem'].item() * 1024 * 1024 * 1024)))

print("%s opmem: %s" % (xs[0], fmtfn(prune['opmem'].item() * 1024 * 1024 * 1024)))
print("%s opmem: %s" % ("partial", fmtfn(partial['opmem'].item() * 1024 * 1024 * 1024)))
print("%s opmem: %s" % (xs[1], fmtfn(full['opmem'].item() * 1024 * 1024 * 1024)))

print("%s vmrss: %s" % (xs[0], fmtfn(prune['vmrss'].item() * 1024 * 1024 * 1024)))
print("%s vmrss: %s" % ("partial", fmtfn(partial['vmrss'].item() * 1024 * 1024 * 1024)))
print("%s vmrss: %s" % (xs[1], fmtfn(full['vmrss'].item() * 1024 * 1024 * 1024)))

#
# throughput achieved
#

tys = []
for x in xs:
    if x == "Noria":
        achieved = prune['achieved'].max()
    elif x == "Noria, partial":
        achieved = partial['achieved'].max()
    elif x == "Noria, no partial":
        achieved = full['achieved'].max()
    elif x == "MySQL":
        achieved = common.mysql_experiments['achieved'].max()
    tys.append(achieved)

bars = throughput.bar(xticks, tys)
bars[0].set_color(common.colors['evict'])
# bars[1].set_color(common.colors['partial'])
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
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
