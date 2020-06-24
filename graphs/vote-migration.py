#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

# just plot skewed with reuse for now
full = None
partial = None
for exp in common.source['vote-migration']:
    c = exp['configuration']
    if c['reuse'] and c['distribution'] == "skewed" and c['articles'] == 10000000:
        exp['old']['time'] -= exp['migration'][0]
        exp['new']['time'] -= exp['migration'][0]
        exp['hitf']['time'] -= exp['migration'][0]
        if c['partial']:
            partial = exp
        else:
            full = exp

fig, ax = plt.subplots()
# ax.plot(data['old']['time'], data['old']['throughput'], 'o', ms=2, alpha=1, color='#a6cee3', label="Votes")
# ax.plot(data['new']['time'], data['new']['throughput'], 'o', ms=1, alpha=0.5, color='#1f78b4', label="Ratings")
ax.plot(partial['hitf']['time'], partial['hitf']['fraction'] * 100, 'o', color=common.colors['partial'], ms=1, label="Noria (partial)")
ax.plot(full['hitf']['time'], full['hitf']['fraction'] * 100, 'o', color=common.colors['full'], ms=1, label="Noria (full)")
ax.axvline(0, color=common.colors['redis'], label="Migration start")
ax.axvline(full['migration'][1] - full['migration'][0], ls='--', color=common.colors['full'], label="Migration end (full)")
ax.set_xlim(-5, 150)
ax.set_ylim(48, 102)
lgnd = fig.legend(loc=(0.57,0.27))
for handle in lgnd.get_lines():
    handle._legmarker.set_markersize(5.0)
    handle._legmarker.set_alpha(1.0)

ax.set_xlabel("Time after migration [s]")
ax.set_ylabel("New view hit \\%")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
