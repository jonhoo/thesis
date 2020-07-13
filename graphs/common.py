import matplotlib
import matplotlib.pyplot as plt
import pandas as pd

golden_ratio = 1.61803
figwidth = 8.5 / golden_ratio

# bring in aggregated results
from memoize import source

# now, extract and clean up the data

#
# Lobsters
#

# extract
lobsters = source['lobsters-noria'].copy()

# adjust units
lobsters["vmrss"] = lobsters["vmrss"] / (1024 * 1024 * 1024)
lobsters["basemem"] = lobsters["basemem"] / (1024 * 1024 * 1024)
lobsters["opmem"] = lobsters["opmem"] / (1024 * 1024 * 1024)
lobsters["fopmem"] = lobsters["fopmem"] / (1024 * 1024 * 1024)
lobsters["rmem"] = lobsters["rmem"] / (1024 * 1024 * 1024)

# compute derivatives
lobsters["mem"] = lobsters["basemem"] + lobsters["opmem"]

# set up indexes properly
lobsters.sort_index(inplace = True)

# find subset that corresponds to the "main" experiment
lobsters_experiments = lobsters.query('op == "all" & memlimit == 0 & achieved >= 0.95 * requested & mean < 100').groupby([c for c in lobsters.index.names if c not in ["op", "until", "metric"]]).tail(1)

# compute subset of data for memory-limited lobsters
limited_lobsters_scale = lobsters.query('op == "all" & memlimit != 0 & achieved >= 0.95 * requested & mean < 100').reset_index()['scale'].max()
limited_lobsters = lobsters.query('op == "all" & memlimit != 0 & scale == %d' % limited_lobsters_scale).groupby('memlimit').tail(1).reset_index()
limited_lobsters_still_ok = limited_lobsters.query('achieved >= 0.99 * requested')["memlimit"].min()
limited_lobsters = lobsters.query('op == "all" & memlimit == %f & scale == %d' % (limited_lobsters_still_ok, limited_lobsters_scale)).tail(1).copy()
print('Using %.0fMB memory limit as representative for lobsters (%d pages/s)' % (limited_lobsters_still_ok * 1024, limited_lobsters["achieved"].min()))

# find scale that is shared among the most lobsters configurations
data = lobsters_experiments
shared_scale = None
shared_scale_cnt = 0
for scale in data.reset_index()["scale"]:
    r = data.query("scale == %d" % scale)
    if len(r) > shared_scale_cnt or (len(r) == shared_scale_cnt and scale > shared_scale):
        shared_scale_cnt = len(r)
        shared_scale = scale
print('Shared lobsteres scaling factor is %dx (%d/%d rows)' % (shared_scale, shared_scale_cnt, len(data)))

# compute maximum scale across all lobsters experiments
max_scale = lobsters.reset_index()["scale"].max() * 1.1
max_pps = (46.0 / 60.0) * max_scale # BASE_OPS_PER_MIN
print("Max scale is %f (%f pages per second)" % (max_scale, max_pps))

#
# vote
#

# extract
vote = source['vote'].copy()

# adjust units
vote["vmrss"] = vote["vmrss"] / (1024 * 1024 * 1024)
vote["basemem"] = vote["basemem"] / (1024 * 1024 * 1024)
vote["opmem"] = vote["opmem"] / (1024 * 1024 * 1024)
vote["fopmem"] = vote["fopmem"] / (1024 * 1024 * 1024)
vote["rmem"] = vote["rmem"] / (1024 * 1024 * 1024)

# compute derivatives
vote["mem"] = vote["basemem"] + vote["opmem"]
vote["aggmem"] = vote["opmem"] - vote["rmem"]

# set up indexes properly
vote.sort_index(inplace = True)

# find subset that corresponds to the "main" experiment
vote_experiments = vote.query('op == "all" & memlimit == 0 & write_every == 20 & achieved >= 0.95 * target & mean < 50').groupby([c for c in vote.index.names if c not in ["op", "until", "metric"]]).tail(1)

# compute subset of data for memory-limited vote
limited_vote_target = 1600000
limited_vote = vote.query('op == "all" & memlimit != 0 & target == %d' % limited_vote_target).groupby('memlimit').tail(1).reset_index()
limited_vote_still_ok = limited_vote.query('achieved >= 0.99 * target')["memlimit"].min()
limited_vote = vote.query('op == "all" & memlimit == %f & target == %d' % (limited_vote_still_ok, limited_vote_target)).tail(1).copy()
print('Using %.0fMB memory limit as representative for vote (achieved %d ops/s)' % (limited_vote_still_ok * 1024, limited_vote["achieved"].min()))

# find target that is shared among the most lobsters configurations
data = vote_experiments
shared_target = None
shared_target_cnt = 0
for target in data.reset_index()["target"]:
    r = data.query("target == %d" % target)
    if len(r) > shared_target_cnt or (len(r) == shared_target_cnt and target > shared_target):
        shared_target_cnt = len(r)
        shared_target = target
print('Shared vote target is %d ops/s (%d/%d rows)' % (shared_target, shared_target_cnt, len(data)))

# compute maximum scale across all vote experiments
mx1 = vote["achieved"].max()
mx2 = vote.reset_index()["target"].max()
max_target = max([mx1, mx2]) * 1.1
print("Max vote target is", max_target)

#
# redis
#

# extract and tidy
redis = source['redis'].copy()
redis.sort_index(inplace = True)

# find subset that corresponds to the "main" experiment
redis_experiments = redis.query('op == "all" & write_every == 1000 & achieved >= 0.95 * target & mean < 50').groupby([c for c in redis.index.names if c not in ["op", "until", "metric"]]).tail(1)

# compute maximum scale across all redis experiments
mx1 = redis["achieved"].max()
mx2 = redis.reset_index()["target"].max()
max_redis_target = max([mx1, mx2]) * 1.1
print("Max redis target is", max_redis_target)

#
# mysql
#

# extract and tidy
mysql = source['mysql'].copy()
mysql.sort_index(inplace = True)

# find subset that corresponds to the "main" experiment
mysql_experiments = mysql.query('op == "all" & achieved >= 0.95 * requested & mean < 100').groupby([c for c in mysql.index.names if c not in ["op", "until", "metric"]]).tail(1)

# compute maximum scale across all mysql experiments
mx1 = mysql["achieved"].max()
mx2 = mysql.reset_index()["requested"].max()
max_mysql_target = max([mx1, mx2]) * 1.1
print("Max mysql target is", max_mysql_target)

#
# next, set up general matplotlib styles so all figures look the same.
#

matplotlib.style.use('ggplot')
matplotlib.rc('font', family='serif', size=11)
matplotlib.rc('text.latex', preamble=['\\usepackage{mathptmx}'])
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
        return '%1.0fGB' % (b / 1024 / 1024 / 1024)
    if b >= 1024 * 1024:
        return '%1.0fMB' % (b / 1024 / 1024)
    if b >= 1024:
        return '%1.0fkB' % (b / 1024)
    return '%1.0fb' % b

# https://colorbrewer2.org/#type=qualitative&scheme=Paired&n=6
colors = {
    'full': '#a6cee3',
    'partial': '#1f78b4',
    'evict': '#33a02c',
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
