#!/usr/bin/env python3

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

# just plot skewed with reuse for now
full = None
partial = None
for exp in common.load('vote-migration'):
    c = exp['configuration']
    if c['reuse'] and c['distribution'] == "skewed" and c['articles'] == 10000000:
        exp['old']['time'] -= exp['migration'][0]
        exp['new']['time'] -= exp['migration'][0]
        exp['hitf']['time'] -= exp['migration'][0]
        if c['partial']:
            partial = exp
        else:
            full = exp

fig, (reads, writes) = plt.subplots(2, 1, sharex = True)

reads.plot(partial['hitf']['time'], partial['hitf']['fraction'] * 100, 'o', color=common.colors['noria'], ms=1, label="Noria")
reads.plot(full['hitf']['time'], full['hitf']['fraction'] * 100, 'o', color=common.colors['full'], ms=1, label="Noria without partial")
reads.axvline(0, color=common.colors['mysql'], label = "New view added")
reads.axvline(full['migration'][1] - full['migration'][0], ls='--', color=common.colors['durable'])
reads.set_xlim(-2, 62)
reads.set_ylim(-5, 105)

reads.set_ylabel("New view hit \\%")
lgnd = reads.legend(loc="lower right")
for handle in lgnd.get_lines():
    handle._legmarker.set_markersize(5.0)
    handle._legmarker.set_alpha(1.0)

def collapse(data):
    old = 0
    new = 0
    old_i = 0
    new_i = 0
    out = {
        'time': [],
        'throughput': [],
    }
    while True:
        if old_i < len(data['old']['time']) and (new_i >= len(data['new']['time']) or data['old']['time'][old_i] <= data['new']['time'][new_i]):
            old = data['old']['throughput'][old_i]
            out['time'].append(data['old']['time'][old_i])
            out['throughput'].append(new + old)
            old_i += 1
        elif new_i < len(data['new']['time']):
            new = data['new']['throughput'][new_i]
            out['time'].append(data['new']['time'][new_i])
            out['throughput'].append(new + old)
            new_i += 1
        else:
            break
    return out

partial_d = collapse(partial)
full_d = collapse(full)
writes.plot(partial_d['time'], partial_d['throughput'], 'o', ms=1, color=common.colors['noria'])
writes.plot(full_d['time'], full_d['throughput'], 'o', ms=1, color=common.colors['full'])
writes.axvline(0, color=common.colors['mysql'])
writes.axvline(full['migration'][1] - full['migration'][0], ls='--', color=common.colors['durable'])
writes.set_xlim(-2, 62)
writes.yaxis.set_major_formatter(common.kfmt)
writes.set_ylabel("Writes/s")
writes.set_xlabel("Time after migration [s]")

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[1]), format="pdf")
# for thesis presentation:
# plt.savefig("{}.png".format(sys.argv[1]), format="png", dpi=256)
