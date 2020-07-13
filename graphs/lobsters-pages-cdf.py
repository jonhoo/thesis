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
ops = [
    "story",
    "frontpage",
    "user",
    "comments",
    "recent",
    "comment_vote",
    "story_vote",
    "comment",
    "login",
    "submit",
    "logout",
]
# only show third quartile
pcts = [x for x in range(10, 75, 10)] + [x for x in range(75, 101, 1)]

lobsters_noria_fn = re.compile("lobsters-direct((?:_)\d+)?(_full)?-(\d+)-(\d+)m.log")
for path in glob(os.path.join(sys.argv[2], 'lobsters-direct*.log')):
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
    scale = int(match.group(3))
    memlimit = float(int(match.group(4)))

    if shards != 0 or scale != 4000 or memlimit != 0:
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

    # time to fetch the cdf
    hist_path = os.path.splitext(path)[0] + '.hist'
    hreader = HistogramLogReader(hist_path, HdrHistogram(1, 60000000, 3))
    histograms = {}
    last = 0
    opi = 0
    while True:
        hist = hreader.get_next_interval_histogram()
        if hist is None:
            break
        if hist.get_start_time_stamp() < last:
            # next operation!
            opi += 1
            pass

        last = hist.get_start_time_stamp()
        if hist.get_tag() != "processing":
            continue

        time = hist.get_end_time_stamp() - hreader.base_time_sec * 1000.0

        if time != 256000:
            # only consider steady-state
            continue

        histograms[ops[opi]] = hist

    df = pd.DataFrame()
    for op, hist in histograms.items():
        row = {
            "op": op,
            "partial": partial,
            "achieved": generated,
        }

        for pct in pcts:
            latency = hist.get_value_at_percentile(pct)
            row["pct"] = pct
            row["latency"] = latency / 1000.0
            df = df.append(row, ignore_index=True)

    data = pd.concat([data, df])

data = data.set_index(["op", "pct"]).sort_index()

fig, ax = plt.subplots()
labels = {
    'story': 'View Story',
    'frontpage': 'Frontpage',
    'comment_vote': 'Vote for Comment',
    'story_vote': 'Vote for Story',
    'submit': 'Submit Story',
}
for op in ["story", "frontpage", "comment_vote", "story_vote", "submit"]:
    d = data.query('partial == True & op == "%s"' % op).reset_index()
    #if limit == 0:
    #    partial = d.query("partial == True")
    #    full = d.query("partial != True")
    #    ax.plot(partial["latency"], partial["pct"], color = 'black', lw=1, alpha = 0.8, ls = "-", label = "unlimited")
    #    ax.plot(full["latency"], full["pct"], color = 'black', lw=1, alpha = 0.8, ls = "--", label = "full")
    ax.plot(d["latency"], d["pct"], label = labels[op])
ax.set_ylabel("CDF")
ax.set_xlabel("Latency [ms]")
ax.set_xscale('log')
ax.set_xlim(0.1, 10000)
ax.legend()

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[3]), format="pdf")
