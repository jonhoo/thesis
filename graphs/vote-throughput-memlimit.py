#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

vote = common.load('vote', only_good = False)

# first, find smallest supported memlimit at 1M
low = vote.query('op == "all" & until == 256')
low = low.query('write_every == 100 & distribution == "skewed"')
low = low.query('memlimit != 0 & target == 1000000 & metric == "sojourn"')
low = low.query('achieved >= 0.99 * target & p95 < 20')
low_mem = low.reset_index()['memlimit'].min()
print('Using %s memory limit as representative for vote' % (common.bts(low_mem * 1024 * 1024 * 1024)))
low = low.query('memlimit == %f' % low_mem)
low_opmem = low['opmem'].item()
print(low_opmem)

fig, ax = plt.subplots()
data = vote.query('op == "all" & clients == 4 & write_every == 100 & until == 256 & distribution == "skewed" & metric == "sojourn"').sort_index().reset_index()
limits = data.groupby('memlimit').tail(1)
limits = [l for l in limits["memlimit"]]
limits.sort()
print(limits)
limits = [256 / 1024.0,  320 / 1024.0, 384 / 1024.0, 448 / 1024.0]
colors = common.memlimit_colors(len(limits))
limits.sort()
limits = limits + [0]
i = 0
fopmem = data.query('memlimit == 0 & target == 1000000 & partial == False')['opmem'].item()
print('vote @ 1M, limit %s: opmem / full opmem = %.1f%%' % (common.bts(low_mem * 1024 * 1024 * 1024), 100.0 * low_opmem / fopmem))
for limit in limits:
    d = data.query('memlimit == %f' % limit).reset_index()
    if limit == 0:
        dd = d.query("partial == True")
        # we need to make sure we measure the memory use
        # at the same throughput level for all the lines.
        opmem = dd.query('target == 1000000')["vmrss"].item()
        ax.plot(dd["achieved"], dd["p95"], '.-', color = 'black', label = "%s (no eviction)" % common.bts(opmem))
    else:
        opmem = d.query('target == 1000000')["vmrss"].item()
        ax.plot(d["achieved"], d["p95"], '.-', color = colors[i], label = '%s' % (common.bts(opmem)))
        i += 1
    opmem = d.query('partial == True & target == 1000000')['opmem'].item()
    print('vote @ 1M, limit %s: opmem / full opmem = %.1f%%' % (common.bts(limit * 1024 * 1024 * 1024) if limit != 0 else "none", 100.0 * opmem / fopmem))

ax.xaxis.set_major_formatter(common.kfmt)
ax.set_ylim(0, 20)
# leave some space for legend:
ax.set_xlim(0, 4000000 * 1.2)
ax.legend(title = "VmRSS @ 1M/s")

ax.set_xlabel("Achieved throughput [requests per second]")
ax.set_ylabel("95-th \\%-ile latency [ms]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
