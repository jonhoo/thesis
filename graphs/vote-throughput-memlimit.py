#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

vote = common.load('vote', only_good = False)

fig, ax = plt.subplots()
base = vote.query('op == "all" & clients == 4 & write_every == 100 & until == 256 & distribution == "skewed" & metric == "sojourn"')
data = base.query('durable == False').sort_index().reset_index()
limits = data.groupby('memlimit').tail(1)
limits = [l for l in limits["memlimit"]]
limits = [448 / 1024.0, 384 / 1024.0, 320 / 1024.0, 256 / 1024.0]
colors = common.memlimit_colors(len(limits))
i = 0
dur = base.query('durable == True & target == 1000000 & memlimit == 0.5')['vmrss'].item()
no_dur = base.query('durable == False & target == 1000000 & memlimit == 0.5')['vmrss'].item()
delta = no_dur - dur
fopmem = data.query('memlimit == 0 & target == 1000000 & partial == False')['opmem'].item()
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    opmem = d.query('target == 1000000')["vmrss"].item()
    ax.plot(d["achieved"], d["p95"], '.-', color = colors[len(limits) - i - 1], label = '%s + %s' % (common.bts(delta), common.bts(opmem - delta)))
    i += 1
    opmem = d.query('partial == True & target == 1000000')['opmem'].item()
    print('vote @ 1M, limit %s: opmem / full opmem = %.1f%%' % (common.bts(limit * 1024 * 1024 * 1024) if limit != 0 else "none", 100.0 * opmem / fopmem))

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_ylim(0, 50)
ax.legend(title = "VmRSS @ 1M/s")

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("95-th \\%-ile latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
