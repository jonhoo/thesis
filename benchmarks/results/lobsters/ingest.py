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

lobsters_noria_fn = re.compile("lobsters-direct((?:_)\d+)?(_full)?(_durable)?-(\d+)-(\d+)m.log")
def parse(df, path):
    match = lobsters_noria_fn.fullmatch(os.path.basename(path))
    if match is None:
        print(match, path)
        return df
    if os.stat(path).st_size == 0:
        print("empty", path)
        return df

    shards = int(match.group(1)) if match.group(1) else 0
    partial = match.group(2) is None
    durable = match.group(3) is not None
    scale = int(match.group(4))
    memlimit = float(int(match.group(5))) / 1024.0 / 1024.0 / 1024.0
    target = 0.0
    generated = 0.0
    sload1 = 0.0
    sload5 = 0.0
    cload1 = 0.0
    cload5 = 0.0
    mem = 0

    data = []
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith("#"):
                if "generated ops/s" in line:
                    generated += float(line.split()[-1])
                elif "target ops/s" in line:
                    target += float(line.split()[-1])
                elif "server load" in line:
                    fields = line.split()
                    sload1 += float(fields[-2])
                    sload5 += float(fields[-1])
                elif "client load" in line:
                    fields = line.split()
                    cload1 += float(fields[-2])
                    cload5 += float(fields[-1])
                elif "server memory" in line:
                    mem += float(line.split()[-1]) * 1024
            else:
                # we'll get cdfs straight from the histograms
                # fields = line.split()
                # op = fields[0]
                # metric = fields[1]
                # pct = int(fields[2])
                # ms = int(fields[3])
                # data.append((op, metric, pct, ms))
                pass

    data = timelines(path)
    if data is None:
        print("skipping file without histograms", path)
        return df

    stats = mem_stats(path)
    if stats is None:
        print("skipping file without stats", path)
        return df
    ndomains, base_mem, other_mem, reader_mem, full_op_mem = stats

    meta = {
        'scale': scale,
        'partial': partial,
        'durable': durable,
        'memlimit': memlimit,

        'requested': target,
        'achieved': generated,

        'ndomains': ndomains,
        'sload1': sload1,
        'sload5': sload5,
        'cload1': cload1,
        'cload5': cload5,
        'vmrss': mem,
        'basemem': base_mem,
        'opmem': other_mem,
        'rmem': reader_mem,
        'fopmem': full_op_mem,
    }
    for (k, v) in meta.items():
        data[k] = v


    # get string types right
    data["op"] = data["op"].astype("string")

    # set the correct index
    data.set_index(["scale", "partial", "durable", "memlimit", "op", "until", "metric"], inplace=True)
    return df.append(data)

def mem_stats(log_path):
    stats_path = os.path.splitext(log_path)[0] + '-statistics.json'
    if os.stat(stats_path).st_size == 0:
        print("empty stats", stats_path)
        return None

    with open(stats_path, 'r') as f:
        stats = json.load(f)
    domains = stats["domains"]

    ndomains = len(domains)
    base_mem = 0
    other_mem = 0
    reader_mem = 0
    full_op_mem = 0
    for domain, dinfo in domains.items():
        for node, ninfo in dinfo[1].items():
            if ninfo["desc"] == "B":
                base_mem += ninfo["mem_size"]
            else:
                if type(ninfo["materialized"]) is dict and "Partial" in ninfo["materialized"]:
                    pass
                else:
                    full_op_mem += ninfo["mem_size"]

                if ninfo["desc"] == "reader node":
                    reader_mem += ninfo["mem_size"]
                    other_mem += ninfo["mem_size"]
                else:
                    other_mem += ninfo["mem_size"]
    return (ndomains, base_mem, other_mem, reader_mem, full_op_mem)

def extract_hist(log_path, *args):
    if "lobsters" in os.path.basename(log_path):
        # can't glob here, since there's no separator, and 1000* would catch 10000.
        hist_paths = [os.path.splitext(log_path)[0] + '.hist']
    else:
        hist_paths = glob(os.path.splitext(log_path)[0] + '-client*.hist')

    hist_paths = [p for p in hist_paths if os.path.exists(p) and os.stat(p).st_size != 0]
    if len(hist_paths) == 0:
        return None

    extract_hist_path = os.path.join(os.path.dirname(os.path.realpath(__file__)), "..", "..", "..", "graphs", "extract-hist")
    extract_hist_cargo = os.path.join(extract_hist_path, "Cargo.toml")
    # print(" ".join(["cargo", "r", "--release", "--manifest-path", extract_hist_cargo, "--", *args, *hist_paths]))
    cdf = subprocess.run(["cargo", "r", "--release", "--manifest-path", extract_hist_cargo, "--", *args, *hist_paths], capture_output=True, text=True)
    return pd.read_table(io.StringIO(cdf.stdout))

def cdfs(log_path):
    cdf = extract_hist(log_path)
    if cdf is None:
        return None

    # flip processing/sojourn to be columns
    cdf = cdf.set_index(["op", "pct", "metric"])["time"].unstack()
    cdf = cdf.rename_axis(columns = None).reset_index()
    return cdf

def timelines(log_path):
    return extract_hist(log_path, "--timeline")

if __name__ == '__main__':
    results = pd.DataFrame()

    for experiment in glob('*.log'):
        results = parse(results, experiment)

    with open('parsed.pickle', 'wb') as f:
        pickle.dump(results, f)
