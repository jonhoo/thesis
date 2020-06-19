#!/usr/bin/env python

import common
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
import sys

# memory use at various lobsters scales
data = common.lobsters_experiments.query('scale == %d' % (common.shared_scale)).reset_index()
if common.shared_scale == common.limited_lobsters_scale:
    limited = common.limited_lobsters.reset_index()
    limited.partial = "prune"
    data = pd.concat([data.reset_index(), limited])
else:
    print('No pruning result at this scale')
data = data.set_index(["scale", "partial"])

fig, ax = plt.subplots()

# plot the "bottom" part of the bars
xs = ["Full", "Partial", "w/ eviction"]
xticks = [x for x in range(len(xs))]
ys = [data.query("partial == False")["fopmem"].item(),
      data.query("partial == True")["fopmem"].item(),
      data.query("partial == 'prune'")["fopmem"].item()]
bars = ax.bar(xticks, ys)
bars[0].set_color(common.colors['full'])
bars[1].set_color(common.colors['full'])
bars[2].set_color(common.colors['full'])

# plot the "top" part of the bars
tops = [data.query("partial == False")["opmem"].item() - data.query("partial == False")["fopmem"].item(),
      data.query("partial == True")["opmem"].item() - data.query("partial == True")["fopmem"].item(),
      data.query("partial == 'prune'")["opmem"].item() - data.query("partial == 'prune'")["fopmem"].item()]
bars = ax.bar(xticks, tops, bottom=ys)
bars[0].set_color(common.colors['full'])
bars[1].set_color(common.colors['partial'])
bars[2].set_color(common.colors['evict'])

ax.set_xticks(xticks)
ax.set_xticklabels(xs)
ax.set_ylim(0, data["opmem"].max() * 1.2) # also fit labels over bars

ax.set_ylabel("memory use [GB]")

def autolabel(rects):
    """Attach a text label above each bar in *rects*, displaying its height."""
    for rect in rects:
        height = rect.get_height()
        ax.annotate('%.1fGB' % (rect.get_y() + height),
                    xy=(rect.get_x() + rect.get_width() / 2, rect.get_y() + height),
                    xytext=(0, 3),  # 3 points vertical offset
                    textcoords="offset points",
                    ha='center', va='bottom')


autolabel(bars)

fig.tight_layout()
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf")
