#!/usr/bin/env python

from glob import glob
import os
import pandas as pd
import subprocess
import re
import io
import json

def ingest(in_dir="."):
    results = {
        'vote': pd.DataFrame(),
        'redis': pd.DataFrame(),
        'vote-migration': [],
        'lobsters-noria': pd.DataFrame(),
    }

    for experiment in glob(os.path.join(in_dir, '*.log')):
        base = os.path.basename(experiment)
        print(base)
        if base.startswith("vote-"):
            vote_migration(results['vote-migration'], experiment)
        elif base.startswith("redis."):
            results['redis'] = redis(results['redis'], experiment)
        elif base.startswith("full.") or base.startswith("partial."):
            results['vote'] = vote(results['vote'], experiment)
        elif base.startswith("lobsters-"):
            results['lobsters-noria'] = lobsters_noria(results['lobsters-noria'], experiment)
        else:
            print("unrecognized log file: %s" % (base))

    # in vote, partial, distribution, reuse, op, pct, and client are categorical
    # results["vote"]["partial"] = results["vote"]["partial"].astype('category')
    # results["vote"]["reuse"] = results["vote"]["reuse"].astype('category')
    # results["vote"]["distribution"] = results["vote"]["distribution"].astype('category')
    # results["vote"]["op"] = results["vote"]["op"].astype('category')
    # results["vote"]["pct"] = results["vote"]["pct"].astype('category')
    # results["vote"]["client"] = results["vote"]["client"].astype('category')

    # in lobsters, partial, op, pct, and client are categorical
    # results["lobsters-noria"]["partial"] = results["lobsters-noria"]["partial"].astype('category')
    # results["lobsters-noria"]["op"] = results["lobsters-noria"]["op"].astype('category')
    # results["lobsters-noria"]["pct"] = results["lobsters-noria"]["pct"].astype('category')

    return results

vote_migration_fn = re.compile("vote-((?:no-)?partial)-(stupid|reuse)-([\d.]+)M.(uniform|zipf1.08).log")
def vote_migration(df, f):
    match = vote_migration_fn.fullmatch(os.path.basename(f))
    if match is None:
        print(match, f)
        return

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
        'migration': migration
    })

redis_fn = re.compile("redis\.(\d+)a\.(\d+)t\.(\d+)r\.(\d+)c(\.\d+m)?\.(uniform|skewed)\.log")
def redis(df, path):
    match = redis_fn.fullmatch(os.path.basename(path))
    if match is None:
        print(match, path)
        return df
    if os.stat(path).st_size == 0:
        print("empty", path)
        return df

    articles = int(match.group(1))
    target = int(match.group(2))
    write_every = int(match.group(3))
    clients = int(match.group(4))
    distribution = match.group(6)
    generated = 0.0
    actual = 0.0
    sload1 = 0.0
    sload5 = 0.0
    cload1 = 0.0
    cload5 = 0.0

    client = 0
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith("#"):
                if "generated ops/s" in line:
                    generated += float(line.split()[-1])
                    client += 1
                elif "actual ops/s" in line:
                    actual += float(line.split()[-1])
                elif "server load" in line:
                    fields = line.split()
                    sload1 += float(fields[-2])
                    sload5 += float(fields[-1])
                elif "client[0] load" in line:
                    fields = line.split()
                    cload1 += float(fields[-2])
                    cload5 += float(fields[-1])
            else:
                # we'll get cdfs straight from the histograms
                pass

    if client == 0:
        print("skipping empty file", path)
        return df

    data = timelines(path)

    meta = {
        'target': target,
        'articles': articles,
        'clients': clients,
        'distribution': distribution,
        'write_every': write_every,

        'generated': generated,
        'achieved': actual,

        'sload1': sload1,
        'sload5': sload5,
        'cload1': cload1,
        'cload5': cload5,
    }

    for (k, v) in meta.items():
        data[k] = v

    # get string types right
    data["op"] = data["op"].astype("string")
    data["distribution"] = data["distribution"].astype("string")

    # set the correct index
    data.set_index(["target", "distribution", "write_every", "clients", "articles", "op", "until", "metric"], inplace=True)
    data = data.sort_index()
    return df.append(data)

vote_fn = re.compile("(full|partial)\.(\d+)a\.(\d+)t\.(\d+)r\.(\d+)c\.(\d+)m\.(uniform|skewed)\.log")
def vote(df, path):
    match = vote_fn.fullmatch(os.path.basename(path))
    if match is None:
        print(match, path)
        return df
    if os.stat(path).st_size == 0:
        print("empty", path)
        return df

    partial = match.group(1) == "partial"
    articles = int(match.group(2))
    target = int(match.group(3))
    write_every = int(match.group(4))
    clients = int(match.group(5))
    memlimit = float(int(match.group(6))) / 1024.0 / 1024.0 / 1024.0
    distribution = match.group(7)
    generated = 0.0
    actual = 0.0
    sload1 = 0.0
    sload5 = 0.0
    cload1 = 0.0
    cload5 = 0.0
    mem = 0

    client = 0
    with open(path, 'r') as f:
        for line in f.readlines():
            if line.startswith("#"):
                if "generated ops/s" in line:
                    generated += float(line.split()[-1])
                    client += 1
                elif "actual ops/s" in line:
                    actual += float(line.split()[-1])
                elif "server load" in line:
                    fields = line.split()
                    sload1 += float(fields[-2])
                    sload5 += float(fields[-1])
                elif "client[0] load" in line:
                    fields = line.split()
                    cload1 += float(fields[-2])
                    cload5 += float(fields[-1])
                elif "server memory" in line:
                    mem += float(line.split()[-1]) * 1024
            else:
                # we'll get cdfs straight from the histograms
                pass

    if client == 0:
        print("skipping empty file", path)
        return df

    data = timelines(path)
    ndomains, base_mem, other_mem, reader_mem, full_op_mem = mem_stats(path)

    meta = {
        'target': target,
        'partial': partial,
        'articles': articles,
        'clients': clients,
        'distribution': distribution,
        'write_every': write_every,
        'memlimit': memlimit,

        'generated': generated,
        'achieved': actual,

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
    data["distribution"] = data["distribution"].astype("string")

    # set the correct index
    data.set_index(["target", "partial", "distribution", "write_every", "clients", "articles", "memlimit", "op", "until", "metric"], inplace=True)
    data = data.sort_index()
    return df.append(data)

lobsters_noria_fn = re.compile("lobsters-direct((?:_)\d+)?(_full)?-(\d+)-(\d+)m.log")
def lobsters_noria(df, path):
    match = lobsters_noria_fn.fullmatch(os.path.basename(path))
    if match is None:
        print(match, path)
        return df
    if os.stat(path).st_size == 0:
        print("empty", path)
        return df

    shards = int(match.group(1)) if match.group(1) else 0
    partial = match.group(2) is None
    scale = int(match.group(3))
    memlimit = float(int(match.group(4))) / 1024.0 / 1024.0 / 1024.0
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
    ndomains, base_mem, other_mem, reader_mem, full_op_mem = mem_stats(path)

    meta = {
        'scale': scale,
        'partial': partial,
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
    data.set_index(["scale", "partial", "memlimit", "op", "until", "metric"], inplace=True)
    return df.append(data)

def mem_stats(log_path):
    stats_path = os.path.splitext(log_path)[0] + '-statistics.json'
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

    extract_hist_path = os.path.join(os.path.dirname(os.path.realpath(__file__)), "extract-hist")
    extract_hist_cargo = os.path.join(extract_hist_path, "Cargo.toml")
    print(["cargo", "r", "--release", "--manifest-path", extract_hist_cargo, "--", *args, *hist_paths])
    cdf = subprocess.run(["cargo", "r", "--release", "--manifest-path", extract_hist_cargo, "--", *args, *hist_paths], capture_output=True, text=True)
    return pd.read_table(io.StringIO(cdf.stdout))

def cdfs(log_path):
    cdf = extract_hist(log_path)

    # flip processing/sojourn to be columns
    cdf = cdf.set_index(["op", "pct", "metric"])["time"].unstack()
    cdf = cdf.rename_axis(columns = None).reset_index()
    return cdf

def timelines(log_path):
    return extract_hist(log_path, "--timeline")

if __name__ == "__main__":
    ingest(".")
