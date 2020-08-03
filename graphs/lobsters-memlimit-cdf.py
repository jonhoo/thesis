#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys, os
import re
from glob import glob
from matplotlib.gridspec import GridSpec

from hdrh.histogram import HdrHistogram
from hdrh.log import HistogramLogReader

plot_scale = 2000
plot_offset = 256000

data = pd.DataFrame()
limits = []
pcts = [1, 5] + [x for x in range(10, 74, 10)] + [x for x in range(74, 101, 2)]

lobsters_noria_fn = re.compile("lobsters-direct((?:_)\d+)?(_full)?(_durable)?-(\d+)-(\d+)m.log")
for path in glob(os.path.join(os.path.dirname(__file__), '..', 'benchmarks', 'results', 'lobsters', '*.log')):
    base = os.path.basename(path)
    match = lobsters_noria_fn.fullmatch(base)
    if match is None:
        print(match, path)
        continue
    if os.stat(path).st_size == 0:
        print("empty", path)
        continue

    shards = int(match.group(1)) if match.group(1) else 0
    partial = match.group(2) is None
    durable = match.group(3) is not None
    scale = int(match.group(4))
    memlimit = float(int(match.group(5))) / 1024.0 / 1024.0 / 1024.0

    if shards != 0 or durable or not partial or scale != plot_scale:
        continue
    
    # check achieved load so we don't consider one that didn't keep up
    target = 0.0
    generated = 0.0
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith("#"):
                if "generated ops/s" in line:
                    generated += float(line.split()[-1])
                elif "target ops/s" in line:
                    target += float(line.split()[-1])
    if generated < 0.95 * target:
        continue
    if memlimit not in limits:
        limits.append(memlimit)

    # time to fetch the cdf
    hist_path = os.path.splitext(path)[0] + '.hist'
    hreader = HistogramLogReader(hist_path, HdrHistogram(1, 60000000, 3))
    histograms = {}
    last = 0
    while True:
        hist = hreader.get_next_interval_histogram()
        if hist is None:
            break
        if hist.get_start_time_stamp() < last:
            # next operation!
            # we're combining them all, so this doesn't matter
            pass
        last = hist.get_start_time_stamp()

        if hist.get_tag() != "sojourn":
            continue

        time = hist.get_end_time_stamp() - hreader.base_time_sec * 1000.0

        if time != plot_offset:
            # only consider steady-state
            continue

        # collapse latencies for all pages
        if time in histograms:
            histograms[time].add(hist)
        else:
            histograms[time] = hist

    df = pd.DataFrame()
    for time, hist in histograms.items():
        row = {
            "memlimit": memlimit,
            "partial": partial,
            "achieved": generated,
        }

        for pct in pcts:
            latency = hist.get_value_at_percentile(pct)
            row["pct"] = pct
            row["latency"] = latency / 1000.0
            df = df.append(row, ignore_index=True)

    data = pd.concat([data, df])

data = data.set_index(["memlimit", "pct"]).sort_index()

fig = plt.figure(constrained_layout=True)
gs = GridSpec(2, 1, figure=fig, height_ratios = [0.8, 0.2])
hi = fig.add_subplot(gs[0, :])
lo = fig.add_subplot(gs[1, :])
limits.sort()
limits = [512, 256, 128, 96]
limits.sort()
colors = common.memlimit_colors(len(limits))
limits = limits + [0]
i = 0
base = common.load('lobsters', only_good = False)
for limit in limits:
    limit /= 1024.0
    d = data.query('memlimit == %f' % limit).reset_index()
    opmem = base.query('until == 1 & partial == True & scale == %d & memlimit == %f' % (plot_scale, limit))['vmrss'].max()
    if limit == 0:
        partial = d.query("partial == True")
        # full = d.query("partial == False")
        opmem_full = base.query('until == 1 & partial == False & scale == %d & memlimit == 0' % (plot_scale))['vmrss'].max()
        lo.plot(partial["latency"], partial["pct"], color = 'black', ls = "-", label = '%s (no eviction)' % (common.bts(opmem)))
        hi.plot(partial["latency"], partial["pct"], color = 'black', ls = "-", label = '%s (no eviction)' % (common.bts(opmem)))
        # ax.plot(full["latency"], full["pct"], color = 'black', ls = "--", label = '%s (no partial)' % (common.bts(opmem_full)))
    else:
        lo.plot(d["latency"], d["pct"], color = colors[i], label = '%s' % (common.bts(opmem)))
        hi.plot(d["latency"], d["pct"], color = colors[i], label = '%s' % (common.bts(opmem)))
        i += 1

hi.set_ylim(75, 101)
hi.set_yticks([75, 90, 95, 100])
hi.set_ylabel("CDF [\\%]")
hi.set_xscale('log')
hi.set_xlim(4, 100)
hi.set_xticks([5, 10, 20, 50])
hi.set_xticklabels(["5ms", "10ms", "20ms", "50ms"])
hi.legend(loc = 'lower right')

lo.set_ylim(0, 75)
lo.set_yticks([0, 25, 50, 75])
lo.set_xlim(1, 8)
lo.set_xticks([1, 2, 4, 6, 8])
lo.set_xticklabels(["1ms", "2ms", "4ms", "6ms", "8ms"])
lo.set_xlabel("Page latency")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
