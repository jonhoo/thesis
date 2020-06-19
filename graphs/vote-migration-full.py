#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

# just plot skewed partial with reuse for now
data = None
for exp in common.source['vote-migration']:
    c = exp['configuration']
    if not c['partial'] and c['reuse'] and c['distribution'] == "skewed" and c['articles'] == 10000000:
        data = exp
        break

data['old']['time'] -= data['migration'][0]
data['new']['time'] -= data['migration'][0]
data['hitf']['time'] -= data['migration'][0]

fig, ax1 = plt.subplots()
ax2 = ax1.twinx()
ax1.plot(data['new']['time'], data['new']['throughput'], 'o', ms=1, alpha=1, color='#a6cee3', label="New writes")
ax1.plot(data['old']['time'], data['old']['throughput'], 'o', ms=1, alpha=1, color='#1f78b4', label="Old writes")
ax1.axvline(0, color='#d95f02')
ax1.axvline(data['migration'][1] - data['migration'][0], color='#d95f02')
ax2.plot(data['hitf']['time'], data['hitf']['fraction'] * 100, 'o', color='#33a02c', ms=2, alpha=0.5, label="Hit \\%")
ax2.set_ylim(0, 100 * 1.1)
ax2.grid(None)
ax1.yaxis.set_major_formatter(common.kfmt)
ax1.set_xlim(-20, 90)
ax1.set_ylim(0, 500000 * 1.1)
fig.legend()

ax1.set_xlabel("Time since migration [s]")
ax1.set_ylabel("Throughput [writes per second]", color='#1f78b4')
ax1.tick_params(axis='y', labelcolor='#1f78b4')
ax2.set_ylabel("Hit \\%", color='#33a02c')
ax2.tick_params(axis='y', labelcolor='#33a02c')

plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf", bbox_inches="tight", pad=0.001)
