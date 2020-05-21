#!/usr/bin/env python

import common
import gflags
import matplotlib
import matplotlib.gridspec as gridspec
from matplotlib.ticker import AutoMinorLocator
matplotlib.use('agg')

import matplotlib.pyplot as plt
import sys, os

FLAGS = gflags.FLAGS
gflags.DEFINE_bool('paper_mode', False, 'Adjusts the size of the plots for paper.')
gflags.DEFINE_string('new_reads', 'non-blocking new reads', 'Name of new reads.')
gflags.DEFINE_string('output', 'migration', 'Output file prefix.')

try:
    sys.argv = FLAGS(sys.argv)
except gflags.FlagsError as e:
    print(e)
    sys.exit(1)

output_file = FLAGS.output
input_files = sys.argv[1:]

common.setup(FLAGS.paper_mode)

# we're given several input files
# in each one, there'll be three primary line types of interest:
# <time> OLD: <throughput> # writes/s for puts to old base
# <time> NEW: <throughput> # writes/s for puts to new base
# <time> HITF: <fraction>  # fraction of reads that succeeded in past second

max_time = 0
data = {}
migration = 0
for input_file in input_files:
    tx = {
        'AGG': ([], []),
        'HITF': ([], []),
        'DONE': 0,
    }

    pstat = "OLD"
    for l in open(input_file).readlines():
      fields = [x.strip() for x in l.split(" ")]
      time = float(fields[0]) / 1000000000.0
      max_time = max(time, max_time)
      stat = fields[1]
      if stat not in ["HITF", "OLD", "NEW", "MIG"]:
        continue

      if stat == "MIG":
        if fields[2] == "START":
          migration = time
        elif fields[2] == "FINISHED":
          tx['DONE'] = time
        continue

      value = float(fields[2])
      if stat == pstat or stat == "OLD":
        tx["AGG"][0].append(time)
        tx["AGG"][1].append(value)
        pstat = stat
      elif stat == "NEW":
        tx["AGG"][1][-1] += value
        pstat = "NEW"
      else:
        tx[stat][0].append(time)
        tx[stat][1].append(value)
    name = os.path.basename(input_file)
    name = name.replace('.log', '')
    name = name.replace('vote-', '')
    name = name.replace('-10M', '')
    name = name.replace('-5M', '')
    name = name.replace('-2M', '')
    name = name.replace('-10k', '')
    data[name] = tx

order = [
    'empty',
    'partial-reuse.zipf1.08',
    'partial-reuse.uniform',
    'no-partial-stupid.zipf1.08',
    'no-partial-stupid.uniform',
    'no-partial-reuse.uniform',
    'no-partial-reuse.zipf1.08',
    'partial-stupid.uniform',
    'partial-stupid.zipf1.08',
]

i = 0
max_time = min(max_time, 395)
max_time -= migration
for name in order:
    if name not in data:
        continue

    if FLAGS.paper_mode:
        gs = gridspec.GridSpec(2, 1, height_ratios=[3, 1], hspace=0.00)
    else:
        gs = gridspec.GridSpec(2, 1, height_ratios=[4, 1], hspace=0.00)
    samples = data[name]

    for j in range(len(samples["AGG"][0])):
        samples["AGG"][0][j] -= migration
    for j in range(len(samples["HITF"][0])):
        samples["HITF"][0][j] -= migration
    samples['DONE'] -= migration

    # main plot
    ax = plt.subplot(gs[0])

    # nicer ticks
    ax.spines["top"].set_visible(False)
    #ax.get_yaxis().tick_left()
    mkfunc = lambda x, pos: '%1.1fM' % (x * 1e-6) if x >= 1e6 else '%1.0fK' % (x * 1e-3) if x >= 1e3 else '%1.0f' % x
    mkformatter = matplotlib.ticker.FuncFormatter(mkfunc)
    ax.yaxis.set_major_formatter(mkformatter)
    ax.set_xticklabels([])
    #ax.set_yticks([0, 25000, 50000])
    ax.yaxis.set_minor_locator(AutoMinorLocator(4))
    ax.xaxis.set_major_locator(plt.NullLocator())

    # show where migration started and finished
    ax.axvline(0, color='0.6')
    ax.axvline(samples['DONE'], color='0.6')

    # add fake bar
    ax.bar(max_time, -1000, color=common.colors['write'], label="Total write throughput", lw=0)
    if FLAGS.paper_mode:
        ax.bar(max_time, -1000, color=common.colors['read'], label=("\\%% %s" % FLAGS.new_reads), lw=0)
    else:
        #ax.bar(max_time, -1000, color=common.colors['read'], label=("%% %s" % FLAGS.new_reads), lw=0)
        pass

    # "main" plot should have old+new write throughput
    if FLAGS.paper_mode:
        ax.plot(samples["AGG"][0], samples["AGG"][1],
                mfc='none',
                mec=common.colors['write'],
                marker="+",
                ms=1.5,
                linestyle='None'
        )
    else:
        ax.plot(samples["AGG"][0], samples["AGG"][1],
                mfc=common.colors['write'],
                mec=common.colors['write'],
                marker=".",
                ms=6,
                linestyle='None',
        )

    # administrativia
    if not FLAGS.paper_mode:
        ax.legend(
            frameon=False,
            ncol=2,
            loc="upper center",
            fontsize='small',
            bbox_to_anchor=(0., 1.10, 0.95, .102),
            #mode="expand",
            borderaxespad=0.,
            #markerscale=5,
            handletextpad=0.3,
            handlelength=1.5,
            columnspacing=0.5,
        )
    elif i == 0:
        ax.legend(
            frameon=False,
            ncol=2,
            loc="upper center",
            fontsize='small',
            bbox_to_anchor=(0., 1.50, 0.95, .102),
            #mode="expand",
            borderaxespad=0.,
            #markerscale=5,
            handletextpad=0.3,
            handlelength=1.5,
            columnspacing=0.5,
        )

    ax.set_ylim(0, 450000)
    ax.set_yticks([0, 100000, 200000, 300000])
    ax.set_xlim(-20, max_time)
    if FLAGS.paper_mode:
        ax.set_ylabel("Throughput")
    else:
        ax.get_xaxis().tick_bottom()
        ax.set_xticks([-15] + [n for n in range(0, 91, 30)])
        ax.set_xticklabels(["-15"] + [str(n) for n in range(0, 91, 30)])
        ax.set_ylabel("Throughput [writes/sec]")

    # bottom plot
    if FLAGS.paper_mode:
        ax = plt.subplot(gs[1])

        # nicer axes and ticks
        ax.get_yaxis().tick_right()
        ax.get_xaxis().tick_bottom()
        ax.spines["top"].set_visible(False)
        ax.set_xticks([-15] + [n for n in range(0, 91, 30)])
        ax.set_yticks([0, 1.0])
        if FLAGS.paper_mode:
            ax.set_yticklabels(["0\%", "100\%"])
        else:
            ax.set_yticklabels(["0%", "100%"])
        ax.yaxis.set_tick_params(labelsize='xx-small')

        # copy the last datapoint to avoid empty space
        hitf = samples["HITF"]
        hitf[0].append(max_time)
        hitf[1].append(hitf[1][-1])

        # show fraction over time
        ax.fill_between(hitf[0], 0, hitf[1], facecolor=common.colors['read'], lw=1.0, color=common.colors['read'])
        ax.set_ylim(0, 1)
        ax.set_xlim(-20, max_time)

        # show migration start and end
        ax.axvline(0, color='0.8')
        ax.axvline(samples['DONE'], color='0.8')

    i += 1
    if i == len(data) or not FLAGS.paper_mode:
        ax.set_xlabel("Time after transition start [sec]")
    else:
        ax.set_xticklabels([])

    plt.savefig("%s_%s.pdf" % (output_file, name), format="pdf", bbox_inches='tight', pad=0.001)
    plt.savefig("%s_%s.png" % (output_file, name), format="png", bbox_inches='tight', pad=0.001, dpi=288)

    plt.clf()
