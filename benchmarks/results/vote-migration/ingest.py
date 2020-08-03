#!/usr/bin/env python

from glob import glob
import os
import pandas as pd
import subprocess
import re
import io
import json
try:
   import cPickle as pickle
except:
   import pickle
import sys

vote_migration_fn = re.compile("vote-((?:no-)?partial)-(stupid|reuse)-([\d.]+)M.(uniform|zipf1.15).log")
def parse(df, f):
    match = vote_migration_fn.fullmatch(os.path.basename(f))
    if match is None:
        print(match, f)
        return df

    partial = match.group(1) == "partial"
    reuse = match.group(2) == "reuse"
    articles = int(float(match.group(3)) * 1000000)
    distribution = match.group(4)

    old = []
    new = []
    hitf = []
    migration = (0, 0)
    with open(f, 'r') as f:
        for line in f.readlines():
            fields = line.split()
            if "OLD" in line:
                time = float(fields[0]) / 1000000000.0
                throughput = float(fields[-1])
                old.append((time, throughput))
            elif "NEW" in line:
                time = float(fields[0]) / 1000000000.0
                throughput = float(fields[-1])
                new.append((time, throughput))
            elif "HITF" in line:
                time = float(fields[0]) / 1000000000.0
                fraction = float(fields[-1])
                hitf.append((time, fraction))
            elif "MIG START" in line:
                time = float(fields[0]) / 1000000000.0
                migration = (time, migration[1])
            elif "MIG FINISHED" in line:
                time = float(fields[0]) / 1000000000.0
                migration = (migration[0], time)

    df.append({
        'old': pd.DataFrame(old, columns = ["time", "throughput"]),
        'new': pd.DataFrame(new, columns = ["time", "throughput"]),
        'hitf': pd.DataFrame(hitf, columns = ["time", "fraction"]),
        'migration': migration,
        'configuration': {
            'partial': partial,
            'reuse': reuse,
            'articles': articles,
            'distribution': "uniform" if distribution == "uniform" else "skewed",
        }
    })

if __name__ == '__main__':
    results = []

    for experiment in glob('*.log'):
        parse(results, experiment)

    with open('parsed.pickle', 'wb') as f:
        pickle.dump(results, f)
