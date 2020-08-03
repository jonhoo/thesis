import os
import matplotlib
import matplotlib.pyplot as plt
import pandas as pd
try:
   import cPickle as pickle
except:
   import pickle
import sys

golden_ratio = 1.61803
figwidth = 8.5 / golden_ratio

def load(d, only_good=True):
    with open(os.path.join(os.path.dirname(__file__), '..', 'benchmarks', 'results', d, 'parsed.pickle'), 'rb') as f:
        df = pickle.load(f)
        if only_good:
            if d.startswith('lobsters'):
                df = df.query('op == "all" & achieved >= 0.99 * requested & mean < 50')
            elif d == "vote-migration":
                pass
            else:
                df = df.query('op == "all" & achieved >= 0.99 * target & mean < 20')
        return df


#
# set up general matplotlib styles so all figures look the same.
#

matplotlib.style.use('ggplot')
matplotlib.rc('font', family='serif', size=11)
matplotlib.rc('text.latex', preamble='\\usepackage{mathptmx}')
matplotlib.rc('text', usetex=True)
matplotlib.rc('figure', figsize=(figwidth, figwidth / golden_ratio))
matplotlib.rc('legend', fontsize=11)
matplotlib.rc('axes', linewidth=1)
matplotlib.rc('lines', linewidth=2)
plt.tick_params(top='off', right='off', which='both')

kfmtfn = lambda x, pos: '%1.1fM' % (x * 1e-6) if x >= 1e6 else '%1.0fk' % (x * 1e-3) if x >= 1e3 else '%1.0f' % x
kfmt = matplotlib.ticker.FuncFormatter(kfmtfn)

def bts(b):
    if b >= 1024 * 1024 * 1024:
        return '%1.1fGB' % (b / 1024 / 1024 / 1024)
    if b >= 1024 * 1024:
        return '%1.0fMB' % (b / 1024 / 1024)
    if b >= 1024:
        return '%1.0fkB' % (b / 1024)
    return '%1.0fb' % b

# https://colorbrewer2.org/#type=qualitative&scheme=Paired&n=6
colors = {
    'full': '#1f78b4',
    'durable': '#a6cee3',
    'noria': '#33a02c',
    'mysql': '#e31a1c',
    'redis': '#fb9a99',
}

# https://colorbrewer2.org/#type=sequential&scheme=RdPu&n=8
def memlimit_colors(n, bright=False):
    if not bright:
        # off by one from the official colors, because #feebe2 is too hard to see
        n += 1

    if n <= 3:
        return ['#c51b8a', '#fa9fb5', '#fde0dd']
    elif n == 4:
        return ['#ae017e', '#f768a1', '#fbb4b9', '#feebe2']
    elif n == 5:
        return ['#7a0177', '#c51b8a', '#f768a1', '#fbb4b9', '#feebe2']
    elif n == 6:
        return ['#7a0177', '#c51b8a', '#f768a1', '#fa9fb5', '#fcc5c0', '#feebe2']
    elif n == 7:
        return ['#7a0177', '#ae017e', '#dd3497', '#f768a1', '#fa9fb5', '#fcc5c0', '#feebe2']
    elif n == 8:
        return ['#7a0177', '#ae017e', '#dd3497', '#f768a1', '#fa9fb5', '#fcc5c0', '#fde0dd', '#fff7f3']
    else:
        return []
