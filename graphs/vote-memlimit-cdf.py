#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys, os
import re
from glob import glob

from hdrh.histogram import HdrHistogram
from hdrh.log import HistogramLogReader

data = pd.DataFrame()
limits = []
# only show third quartile
pcts = [x for x in range(10, 75, 10)] + [x for x in range(75, 101, 1)]

vote_fn = re.compile("(full|partial)\.(\d+)a\.(\d+)t\.(\d+)r\.(\d+)c\.(\d+)m\.(uniform|skewed)\.log")
for path in glob(os.path.join(sys.argv[2], '*.log')):
    base = os.path.basename(path)
    if not (base.startswith('full.') or base.startswith('partial.')):
        continue

    match = vote_fn.fullmatch(base)
    if match is None:
        print(match, path)
        continue
    if os.stat(path).st_size == 0:
        print("empty", path)
        continue

    partial = match.group(1) == "partial"
    articles = int(match.group(2))
    target = int(match.group(3))
    write_every = int(match.group(4))
    clients = int(match.group(5))
    memlimit = float(int(match.group(6)))
    distribution = match.group(7)

    if articles != 5000000 or write_every != 20 or clients != 6 or distribution != "skewed" or target != common.limited_vote_target:
        continue
    
    # check achieved load so we don't consider one that didn't keep up
    actual = 0.0
    generated = 0.0
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith("#"):
                if "generated ops/s" in line:
                    generated += float(line.split()[-1])
                elif "actual ops/s" in line:
                    actual += float(line.split()[-1])
    if actual < 0.95 * target:
        continue
    if memlimit not in limits:
        limits.append(memlimit)

    # time to fetch the cdf
    hist_paths = glob(os.path.splitext(path)[0] + '-client*.hist')
    write = True
    for hist_path in hist_paths:
        hreader = HistogramLogReader(hist_path, HdrHistogram(1, 60000000, 3))
        histograms = {}
        last = 0
        while True:
            hist = hreader.get_next_interval_histogram()
            if hist is None:
                break
            if hist.get_start_time_stamp() < last:
                # next operation (read/write)!
                # we're combining them all, so this doesn't matter
                write = not write
                pass
            last = hist.get_start_time_stamp()

            if hist.get_tag() != "sojourn":
                continue
            if write:
                continue

            time = hist.get_end_time_stamp() - hreader.base_time_sec * 1000.0

            if time != 256000:
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

fig, ax = plt.subplots()
limits.sort()
print(limits)
limits = [512 * 1024 * 1024, 384 * 1024 * 1024, 256 * 1024 * 1024]
colors = common.memlimit_colors(len(limits))
limits.sort()
limits = limits + [0]
i = 0
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    lookup_limit = limit / 1024 / 1024 / 1024
    opmem = common.source['vote'].query('until == 1 & op == "all" & partial == True & articles == 5000000 & write_every == 20 & clients == 6 & distribution == "skewed" & target == %d & memlimit == %f' % (common.limited_vote_target, lookup_limit))['opmem'].max()
    if limit == 0:
        partial = d.query("partial == True")
        full = d.query("partial == False")
        ax.plot(partial["latency"], partial["pct"], color = 'black', ls = "-", label = 'no eviction (%s)' % (common.bts(opmem)))
        # ax.plot(full["latency"], full["pct"], color = 'black', ls = "--", label = "full")
    else:
        ax.plot(d["latency"], d["pct"], color = colors[i], label = '%s limit (%s)' % (common.bts(limit), common.bts(opmem)))
        i += 1
ax.set_ylabel("CDF")
ax.set_xlabel("Latency [ms]")
ax.set_xscale('log')
ax.legend()

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[3]), format="pdf")
