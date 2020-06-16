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
xs = ["Full", "Partial", "Eviction"]
xticks = [x for x in range(len(xs))]
ys = [data.query("partial == False")["opmem"].item(),
      data.query("partial == True")["opmem"].item(),
      data.query("partial == 'prune'")["opmem"].item()]
bars = ax.bar(xticks, ys)
ax.set_xticks(xticks)
ax.set_xticklabels(xs)
bars[0].set_color(common.colors['full'])
bars[1].set_color(common.colors['partial'])
bars[2].set_color(common.colors['evict'])
# ax = data["opmem"].plot.bar(title="Operator state only")
ax.set_ylabel("memory use [GB]")

plt.tick_params(top='off', right='off', which='both')
plt.savefig("{}.pdf".format(sys.argv[2]), format="pdf", bbox_inches="tight", pad=0.001)
