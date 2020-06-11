This thesis is built on the belief that partial materialization is
useful. In this section, I provide experimental and qualitative evidence
that this belief is warranted through analysis of the following
questions:

 1. Why is materialization useful for real-world use-cases?
 2. In what ways are existing materialization techniques lacking?
 3. **How** does partial state improve the usefulness of materialization?
 4. **When** does partial state improve the usefulness of materialization?
 5. What is the cost of using partial state?
 6. How does partial state compare to existing solutions?

I cover these questions in the following sections. The high-level
take-away is that partial state significantly reduces the labor cost and
memory overhead of application caching, is competitive with existing
non-partial solutions, and presents a trade-off between memory use and
increased tail latency.

# Why is materialization useful for real-world use-cases?

Caching
OSDI paper

# In what ways are existing materialization techniques lacking?

Caching is labor intensive + error prone (Facebook memcached paper)

DB view materialization (like DBToaster) "all or nothing"

Commercial DB view materialization limited and slow (ref OSDI paper)

# How does partial state improve the usefulness of materialization?

By folding the cache into the database, database knows how to keep it up
to date — avoids having that logic threaded through the application.

Large parts of the application's dataset and queries are access
infrequently, if at all. We want to devote memory only where it matters.

Fig.: Lobsters memory use full/partial (many things not accessed) and
partial/evict (many things infrequently accessed).

Note that evict is necessary because, over time, more and more of the
tail will be sampled. Without eviction, eventually the entire tail would
be kept in memory (so == full).

Partial state also enables efficient migrations in many cases. Any view
that can be made partial (ref section + later eval) is immediately
available, and does not compute until requested.

Fig.: vote-migration skewed partial vs full

# When does partial state improve the usefulness of materialization?

## When does partial help?

Materialization is generally useful when reads are more common than
writes, since it shifts the computation from reads to writes.
But, without partial state, materialization is an "off/on" option —
either you materialize all the results for a query, or none [TODO: not
quite true — more subtlety here — depends what you mean by "query"
(e.g., parameterized or not)].

Partial state (and caching in general) enables _selective_
materialization. This makes sense if _most_ application accesses are for
a _particular_ subset of the dataset. That is, accesses are _skewed_
towards particular data. In applications with skewed access patterns
(common — ref log-normal + Lobsters above), you can choose to only
materialize commonly-accessed results, rather than all. This uses
potentially far less memory (as indicated above), while still giving you
a speedup for _most_ accesses. The more memory you provide the partial
system with, the more of your tail will be pre-computed, and the more of
your requests will be fast.

Fig.: mem limit vs latency [mean/50/95/99] in Lobsters.

Deciding what memory limit to set is challenging, in part because it
may change over time as the access patterns change, and in part because
it depends on load. The higher the load, the more requests will be in
the tail (and miss). Of course, more requests will be in the head too.

Fig.: throughput vs 95%-ile latency [memlimits] in vote.

For example, for Zipf distribution, we can compute %s:

Tab.: | load | skew | keys hit in 30s | keys hit by 99% | 

This is (again) why eviction is important — without it, partial would approach
full.

Lobsters suggests that significant skew is common. Also ref other
studies on log-normal/zipf/skew.

## When does partial even apply?

 - SELECT COUNT(*) FROM table;
 - Add "karma" query:
   - If already have "karma" column, trivial in DB. But, equivalent in
     Noria to saying that query was there from the start, so no
     migration.
  - If _not_ already have "karma" column, DB has two choices:
    1. Compute column == expensive, and same as Noria would do
    2. Make query compute value on-demand -- same cost, but repeated!
 - TopK

# What is the cost of using partial state?

Migrations maybe? Latency timeline to show that early accesses are slow.
Maybe good place to have comparison with Redis? Maybe show vote uniform
numbers? Talk about "warmup".

Fig.: throughput vs latency w/wo partial in vote.

Note latency diff for lobsters, and that full fell over due to mem, and
no bigger machine w/o more cores available.

Note that the _reason_ vote falls over when it does is single-core write
processing (since only one query, so one data-flow path).

# How does partial state compare to existing solutions?

This is a difficult question to answer. High-performance solutions are
often developed specifically for a given application, and not available
as general-purpose tools (ref facebook memcache, _need cites here_).
And applying the general-purpose tools that _are_ available (memcache,
redis) effectively, requires significant effort on the part of the
application authors (or the evaluators). To manually add caching support
to Lobsters' XX queries, including mitigation, thundering herd
mitigation, and incremental updates would be a massive undetaking.

In some sense, this alone is an argument for Noria's approach. Since the
database uses information it already possesses (in the form of
application queries) to _automatically_ optimize accesses through
materialization, it is relatively easy to take advantage of the benefits
that Noria provides.

Nonetheless, to shed some light on the _absolute_ performance that Noria
provides against a caching system, I include below a performance
comparison between Noria and Redis. To approximate how a carefully
planned and optimized application caching deployment might perform, it
runs a workload that is idealized for a caching system:

 - Every Redis access hits in cache, to emulate perfect thundering herd mitigation and invalidation-avoidance schemes.
 - Nearly all accesses (99.9%) are reads, since writes would be bottlenecked by the backing store.
 - All accesses are for a single integer value, to emulate a system that has perfect cache coverage.
   Request access keys are chosen according to a Zipfian distribution with alpha = 1.08 (moderate skew).
 - Accesses are batched to reduce serialization cost and increase throughput.
   Specifically, reads are `MGET`s, and writes are pipelined `INCRBY`s.

This is not a realistic use of Redis, but it allows us to "assume the
best" about the underlying caching strategy and system. The benchmark
runs for four minutes, and then samples latencies for another two
minutes.

Fig.: throughput vs 95%-ile latency [noria,redis]

Redis is single threaded, which necessarily limits its performance. If
we assume perfect sharding, Redis should be able to support 16 times the
load on a 16-core machine. We see that 16x Redis ~= Noria (??), which
suggests that Noria is competitive with manual caching schemes.
