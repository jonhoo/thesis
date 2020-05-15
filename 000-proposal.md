# Thesis Proposal: Partial Dataflow in Noria

The dataflow architecture has seen a resurgence in recent years, with
systems like Kafka, Spark, Flink, and Naiad all seeing interest from
academia and industry alike. The term "dataflow" covers a wide range of
systems: streaming systems (like Kafka), stream processing systems (like
Spark and Flink), and dataflow computation systems (like Naiad). These
systems vary in their design goals, their intended uses, and their
system architecture. Yet they all share the property that they take data
as _input_, and feed processed data forward through a graph of
_operators_.

This data-flow design is attractive for reactive applications, where
computations are relatively fixed, but the data is changing, since it
clearly designates the data dependencies of the application. Explicitly
modeling the compute and data flow this way also allows dataflow systems
to easily scale to multiple cores or physical hosts; any dataflow edge
can be realized as a function call, a thread synchronization point, or a
network channel, and the model still "works".

These systems all face a decision when it comes to _stateful_ operators
— operators that must maintain some state in order to process their
inputs. To compute the result of a join for example, the values in the
"other side" of the join must be available to the operator that performs
the join. Traditionally, dataflow systems have made one of three
decisions: ignore such operators, make all the state the operator needs
available, or keep only a subset of the state. The third option, often
called, windowing, is a popular option because it lets the system
support stateful operators without keeping vast amounts of state at the
ready just in case the operator needs it. The window will usually keep
only recent data, so that the results of the computation is only
"missing" data that is old anyway.

Each one of these three decisions caters to a particular type of
application. Non-stateful dataflow is useful as a messaging fabric.
Fully stateful dataflow works well for applications that operate over
small data sets, or where the application's working set spans nearly the
entire data set. Windowing is a useful intermediate point for
applications that do not need complete results, and are willing to trade
completeness for performance, such as analytics workloads.

But unfortunately, not all applications fit into one of these
categories. In particular, user-facing applications whose working set is
significantly smaller than their total dataset size are not well served
by these options. Stateless operation is not feasible, since evaluating
the dataflow from scratch each time would incur significant extra
latency. Fully stateful operation is similarly unattractive —
computational resources would need to be provisioned for computing over
all the application's data, even though only a small subset of the
computation output is observed. And windowing can often not be applied
for these applications, since users may request data that lives outside
the window, and that data must still be available.

This thesis presents the Noria dataflow system; a dataflow system that
supports _partially-stateful dataflow_. In Noria, operators act as
though they have access to the full state of their inputs. While in
reality, that state is lazily constructed behind the scenes; a given
piece of the input state for an operator is only produced and stored
when the operator asks for that piece. If an operator only accesses part
of its input state, the remaining parts are not computed or stored.

This approach provides a number of benefits. First, its memory use is
**proportional to the working set** of the application, rather than to
the size of the data. Second, it works for applications that **cannot
use windowing**. Third, it allows the system to **eagerly discard**, and
avoid computation for, data that later operators have never needed, as
long as that data can later be re-produced. And finally, it allows the
application to **selectively evict** from stateful operators as the
working set changes.

Another key advantage of partial state is that it makes it possible to
extend a running dataflow program **lazily**. Noria can cheaply
accommodate new segments of dataflow by instantiating the new dataflow
as initially empty. That new dataflow is then populated through
application activity, rather than by incurring a large upfront cost.

My thesis will cover the design and implementation of partially stateful
dataflow in Noria in detail, including several key components that were
only briefly sketched or not present at all in the earlier [OSDI paper
on Noria][osdi] that I co-authored. I will discuss the specific problems
that arise in depth, and provide solutions to those problems.

## Overview of Noria

Noria implements the partially stateful dataflow model for incremental
view maintenance in databases. It focuses on building a better database
backend for read-heavy applications where a long-running dataflow
program maintains any number of materialized user-defined views,
specified in SQL. Noria uses joint query optimization techniques to find
ways to integrate new views and queries with the running dataflow. The
system is also highly concurrent and distributed, and supports sharding
cliques of operators to share resource costs and increase sustainable
throughput.

Dataflow is a broad term, so I want to take a moment to discuss Noria's
specific dataflow implementation. Noria takes SQL queries from the
application, and folds them into a single, unified dataflow program. The
dataflow is a directed, acyclic graph, with the base tables at the
"top", and application-accessible views at the "bottom". The nodes in
between represent the SQL operators that make up the query of every
view. Reads (`SELECT` queries) access materializations at the leaves
only, while writes (`INSERT`, `UPDATE`, and `DELETE` queries) flow into
the graph at the roots.

After a write has been persisted by the base table operator, it flows
into the dataflow graph as an "update", following the graph's edges.
Every operator the update passes through processes the update according
to the operator semantics, and emits a derived update. Eventually the
update arrives at a leaf view and changes the state visible to reads
through the leaf's materialization. Updates are _signed_ (i.e., they can
be "negative" or "positive"), with negative updates reflecting
revocations of past results, and modifications represented as a
negative-positive update pair.

<!--
My primary work on the paper revolved around the implementation of
partial state, including much of the core dataflow fabric.
-->

Partial state is key to Noria's feasibility — without it, all results
for every prepared query must be computed and maintained, at a
significant memory and compute cost. With partial state, query results
are instead populated on demand, and only parts of the results relevant
to the application's particular query parameters are computed and
maintained. Partial state also enables Noria to implement eviction, so
that the materialization cost is kept low even as the underlying
workload changes.

The essence of the design is to introduce a notion of "missing state" to
the dataflow engine. And alongside it, a mechanism to retroactively, and
efficiently, compute over past inputs to repopulate that missing state
on demand. This mechanism is called the "upquery". An upquery reaches
through the dataflow towards its inputs, and triggers re-processing of
dataflow messages needed to compute missing state. By modeling
re-population of state this way, most of the dataflow can remain
oblivious to the fact that state can be missing. Operators implement
only regular, "forward" data-flow, and the dataflow fabric takes care of
issuing upqueries and re-executing operators over old data as needed.

This design, while alluring, introduces a number of challenges in
practice. Many of these stem from operators that may now need to
communicate with their inputs to populate needed state. Those inputs may
again need to retrieve that state from their inputs, and so on until the
source of the needed data is reached. Upqueries logically flow in the
"opposite" direction of the data, something existing dataflow systems do
not generally allow. Upqueries also race with other upqueries, and with
concurrent modifications propagating through the dataflow, and the
dataflow must be careful to not let permanent inconsistencies arise as a
result of this.

## Thesis: Partially Stateful Dataflow

In my thesis, I propose to give a more complete architecture for
the partially stateful dataflow as implemented in Noria. This includes:

 - a more exhaustive analysis of the primary difficulties in realizing
   partial state in dataflow
 - comprehensive discussion of the solutions to those difficulties
 - an evaluation of the partially stateful model as implemented in Noria

In the following, I go into more detail of what that work will entail.

### Technical Approach

The thesis will build on the work from the 2018 OSDI paper, but go into
more detail specifically about the partially stateful model. I will
approach the work by specifying the key invariants of partially stateful
dataflow, deriving problematic cases that challenge those invariants,
and then outlining practical solutions to those cases. This includes
giving a full explanation for how upqueries are planned, issued, and
executed.

To give some intuition for why this problem is challenging, we first
need to understand what the goal of the system as a whole is.
Ultimately, the partial invariants all serve to maintain one principal
property:

> If data stops flowing into the dataflow, the dataflow will eventually
> quiesce. When it does, for every key in every state, the value for
> that key is either missing, or it reflects the effects of each input
> to the system applied exactly once. A subsequent query for any missing
> key in any materialization populates the state for the missing key
> consistent with the property above for non-missing state.

The intuition here is that Noria must _at least_ eventually do the right
thing. That is, it must make sure that all the data the application
inserts into the dataflow is considered, that none of it is
double-counted, and that no other spurious data is added. Unless, of
course, the application has inserted dataflow operators that
double-count, in which case they should be exactly double-counted.

We want Noria to provide stronger guarantees than eventual consistency
whenever possible and, in the common case, it does. Specifically, for
most queries, Noria ensures that a read from any given view sees
complete query results as of some recent time at each dataflow input.
That is, for a given view, for each input that feeds into that view, the
view reflects a prefix of the data ingested by that input. I call this
_prefix consistency_. Each view is also continuously kept up to date;
any new input is reflected in the view shortly after being ingested,
subject only to the propagation delay in the dataflow.

Noria does not necessarily provide prefix consistency when there are
**multiple** paths from a given dataflow input to a given view, such as
through a self-join. Depending on the precise semantics of the paths,
this can cause a view to briefly reflect **some** of the effects of
newly inserted data, but not all. For example, consider a self-join that
computes a parent-child relationship between records. If the application
removes a record `A`, that dataflow input must be processed along two
edges. When it has been processed by one edge, but no the other, the
downstream view will briefly continue to include `A` as a child, even
though it no longer appears as a parent. This inconsistency is rectified
once the dataflow input is also processed on the second edge.

This problem is not directly related to partial state — Noria exhibits
this behavior when all state is fully materialized. However, partial
state must work in the context of such temporary inconsistencies.
Furthermore, partial state should not exaggerate these problems by
introducing additional inconsistencies.

There are several situations that arise in a real dataflow
implementation that make even this seemingly simple property difficult
to uphold. I sketch the primary ones below, and give brief descriptions
of my proposed solutions. In my thesis, I will go into these in greater
detail. I will also provide a more comprehensive analysis of the
possible inconsistencies that can arise if these situations are not
handled correctly by the partial state logic.

#### Challenge: Partial Eligibility

Partial state only makes sense if populating a particular subset of the
state of an operator is cheaper than populating its entire state. And it
comes with costs of its own. Partial state must track tombstones for
empty, non-missing state, and issuing and responding to upqueries is
slower than regular "forward" dataflow processing.

As a trivial example consider an aggregation that counts the total
number of books in a database. If the application performs a lookup in
this aggregation's state, that lookup has no parameters. As a result, if
the state was partial, and missing, the system's only option is to query
for **all** the books, count them, and then store that count. At that
point, the state is fully materialized. Partial state did not buy us
much here beyond laziness, since the state is only ever empty or full.
In a case like this, it may be faster for the system to eagerly
materialize the aggregation's state to avoid the latency and storage
overheads that partial state introduces.

Partial state is useful when the system can perform "narrower" upqueries.
That is, upqueries that do not query the entire state of ancestors. As
an example, consider an aggregation that counts the number of books in a
given genre. When the application queries the state of this aggregation,
it queries for the count for a **given** genre. The aggregation only
needs to upquery the books for **that** genre, and count those. Only the
counts for frequently queried genres will be materialized and
maintained.

The implementation must intelligently analyze the dataflow to determine
which state should and should not be partially materialized. The first
operator should probably not, whereas the latter should. In addition,
the implementation may need to add supplement indices so that the books
with a given genre can be efficiently queried — if the upquery to find
all books for a given genre is satisfied by a scan over all books, then
upqueries are very expensive, and making the operator partial may not be
worthwhile.

This analysis can get complicated quickly. For example, an operator
cannot be partial if any of its descendants must be fully materialized.
Since a partial operator may discard data early (if processing the data
requires state that is missing), that update will then be perpetually
missing from the downstream full materialization, which violates our
main system property.

Or, consider a variant of the book-by-genre query above, but where the
application queries for genres with a given number of books. While we
still have a parameter to divide the state space, the aggregation has no
efficient way to upquery for "all the books whose genre has N books".
The aggregation is still the same, but the upquery requirements placed
on it are different, and this affects the choice of whether it should be
partial or not.

**Noria solution**: The idea here is to perform _key provenance
analysis_ over all the upqueries an application may issue to the system.
By analyzing the dataflow, we can statically determine the set of all
possible upqueries, and then trace what columns they need to access in
their ancestors. The problematic cases above then become visible as
particular patterns in those key traces. For example, in the
query-by-number book example above, the analysis can conclude that no
full materialization contains the data needed by the upquery, and
therefore it cannot be satisfied efficiently.

#### Challenge: Multi-Ancestor Operators

Operators that have multiple ancestors pose a problem to the partial
model. Consider an identity operator that merely combines the input
streams of its ancestors (i.e., a union). An upquery that crosses this
operator must _split_ its upquery; it must query each ancestor of the
operator, and take the union of the responses to populate missing state.
But when we allow concurrent processing, these responses may be
arbitrarily delayed between the different upquery paths.

Let's examine what happens with a union, U, across two inputs, A and B,
and a single materialized and partial downstream operator C. C discovers
that it needs the state for `k = 1`, and sends an upquery for `k = 1` to
both A and B. A responds first, and C receives that response. It needs
to remember that the missing state is still missing, so that it does not
expose incomplete state downstream (e.g., if it received an upquery for
`k = 1`, it could not reply with **just** A's state). Now imagine that
both A and B send one normal dataflow message each, and that they both
include data for `k = 1`. When these messages reach C, C faces a
dilemma. It cannot drop the messages, since the message from A includes
data that was not included in A's upquery response. If it dropped them,
that data would disappear forever, which violates our primary system
property. But it also cannot apply the messages, since B's message
includes data that will be included in B's eventual upquery response. If
it did, that data would be duplicated.

How upqueries work across multi-ancestor operators depends on the
semantics of that operator. For unions, as we saw above, the upquery
must go to all the ancestors. For joins on the other hand, the upquery
must only go to **one** ancestor. This is because when a join processes
a message from one ancestor, it already queries the "other" ancestor and
thus pulls in any relevant state. In the example above, if U were a
join, then if C sent an upquery to both A and B, the two upquery
responses it received would contain duplicate data. For a symmetric
join, the responses would in fact be identical, whereas for an
asymmetric join (like a left join), they would differ. This suggests
that we must determine the algorithm for upqueries across each type of
multi-ancestor operator separately. Unions, joins, and left joins for
example all have different upquery restrictions.

**Noria solution**: Unions must buffer upquery results until all their
inputs have responded. In the meantime, they must buffer updates for the
buffer upquery keys to ensure that a single, complete, upquery response
is emitted. Joins already contain implied upqueries, so only one side
must be upqueried, with the upqueries to the other side left to the join
itself. Left joins work the same way, but with the additional
restriction that the initial upquery _must_ go the left input.

#### Challenge: Dependent Upqueries

Sometimes, an operator must issue an upquery upstream in order to
satisfy an upquery from downstream. I refer to a recursive upquery like
this as a _dependent_ upquery. Dependent upqueries are not, in and of
themselves, complicated. They function exactly like a regular upquery.
However, it turns out they pose a challenging system design problem.

Upquery responses must, in some sense, be atomic. They must occur at
some single logical point in time with respect to an operator's input
and output streams. Consider what happens if an operator is part-way
through processing an upquery response, and discovers that it must
perform a dependent upquery in order to complete that processing. It may
be a while before the dependent upquery resolves, and in the meantime
the operator needs to decide what to do.

If it blocks waiting for the response to come back, it holds up all
processing of other upqueries and writes. That would not be great. On
the other hand, if it continues processing other inputs, it risks
dropping or duplicating inputs; any part of the upquery response it
produced **before** it found the need for the dependent query still
reflects the state at that point in time. Since it may have processed
writes since then that affect that computed state, the upquery response
would no longer reflect a current, atomic snapshot.

**Noria solution**: Operators issue dependent upqueries only if the need
arises while processing an upquery response. Otherwise, that part of the
current update is discarded. If a dependent upquery must be issued to
complete processing some past upquery response, the response is dropped,
the dependent upquery is issued, and the operator re-tries the original
upquery when the dependent upquery resolves.

#### Challenge: Indirect Dependencies

We have to guarantee that all data relevant to a given state entry
eventually reaches that state. A corollary of this is that we cannot
discard messages that may affect non-missing, downstream state.
Normally, this is the case, since upqueries traverse the dataflow from
the leaves and "up" — if some key `k` is present at an edge down the
graph, it is also present at every materialization above that edge, and
therefore messages with key `k` will not be discarded early.

Unfortunately, this only holds for upqueries where all dependent
upqueries share the same key as the leaf-most upquery. Consider a
dataflow that joins two inputs, `Article` and `User`, on the article's
author field. A downstream operator then issues an upquery for article
number 7. The upquery is issued to `Article`, which produces a message
that contains article number 7 with, say, author "Elena". That message
arrives at the join, which issues a dependent upquery to `User` for
"Elena". When that dependent upquery resolves, the join produces the
final upquery response, and the state for article number 7 is populated
in the downstream materialization.

Next, an editor changes the author for article number 7 to "Talia". This
takes the form of a message with a negative for `[7, "Elena"]` and a
positive for `[7, "Talia"]`. When this message arrives at the join, it
may miss when performing the lookup for "Talia". The join therefore
drops `[7, "Talia"]`, and only the negative for "Elena" propagates to
the downstream materialization. It then marks the state for article
number 7 as empty (though not missing). Any subsequent read for article
number 7 receives an empty response, which violates our primary system
property.

**Noria solution**: Key provenance analysis detects when the dataflow
downstream of an operator has this property. With that information, an
operator knows when it is about to drop an update that _may_ nonetheless
exist in downstream state. It issues an eviction for that state,
ensuring that if the updated state is subsequently needed, it will be
queried for.

#### Challenge: Sharded Upqueries

Noria supports sharding cliques of operators to increase the throughput
of particular sections of the dataflow. Shards of an operator execute in
parallel, without synchronization. Edges that cross from an unsharded
operator to a sharded one split its outgoing updates using hash
partitioning. Edges that cross back have an implicit union injected to
merge the sharded results. Edges that cross from one sharding to a
different sharding are merged and then split again. Upqueries must also
work when Noria decides to shard operators in this way.

Upqueries across a sharding boundary are a complicated affair. The
operator that issues the upquery must determine which shard or shards to
send the upquery to. If it queries multiple shards, the responses from
those shards are subject to the same multi-ancestor issue as unions.
When a response to the upquery comes back, it must be specifically
routed to only the requesting shard, so that it does not accidentally
populate the state of other shards. This logic must work even if
multiple shards issue an upquery for the same key concurrently. Or,
worse yet, if a single upquery must traverse **multiple** sharding
boundaries.

**Noria solution**: Key provenance informs operators whether an
upquery for a given column should be sent to all shards, or just one
shard, of the upquery source. This information, as well as the shard
identifier of the requesting operator, is included in the upquery
itself, and in the eventual response. Sharding unions buffer upquery
responses that originated from more than one shard (like regular
unions). Shard "splitters" ensure that responses only arrive at the
requesting shard using the requestor information in the response.

### System Evaluation

The thesis will include extensive micro and macro benchmarks of the
costs and benefits of introducing partial state to a dataflow system. In
particular, I propose the following experimental targets:

First, **memory use**. These experiments will evaluate the memory
savings that come from using partial state for different workloads. I
would expect to see that partial state allows the memory footprint of
the application to be proportional to its working set size, as opposed
to its data size.

Second, **upquery performance**. These experiments will evaluate the
cost of populating state in response to user reads for different
workloads. This is an important metric, as it directly corresponds to
the latency the application will see when querying data that was
previously not in the working set. Different workloads will exhibit
different latency profiles, since the upquery depth, width, and
complexity will vary.

Third, **steady-state read performance impact**. These experiments will
evaluate the cost of partial state in the steady-state where few or no
upqueries are issued. Ideally, this should not differ significantly from
the steady-state read performance when partial state is disabled. This
experiment exists primarily to evaluate the overheads that partial state
logic may introduce on the read path.

Fourth, **write performance impact**. These experiments will evaluate
the change in write performance as a result of introducing partial
state. The primary focus here is on whether the partial state logic adds
significant overheads to the dataflow processing pipeline. The results
here will likely vary highly depending on the specific dataflow and
workload in question. In skewed dataflow programs with fairly simple
data dependencies, write performance should increase, as unpopular keys
no longer need to be maintained. In dataflow programs with a large
working set relative to the data set, performance is likely to be mostly
the same. For applications that have complex data dependencies, the
bookkeeping needed to ensure that the partial state remains consistent
may introduce a write performance penalty.

And fifth, **overall application impact**. For a complex application
with a wide range of queries and complex access patterns, it may be hard
to predict how partial state will affect the application's performance
and memory use. To estimate this, an end-to-end experiment with a
realistic application and workload is needed. My plan here is to
follow in the footsteps of the OSDI paper and evaluate the "Lobsters"
application, since I already have the queries and a workload generator
for it. This experiment will also help answer the question of what
overhead partial state adds.

### Contributions

The contributions of my thesis, subject to this proposal, will be:

 - An algorithm for implementing upqueries.
 - Support for partial in sharded, complex applications.
 - Key correctness invariants for partially stateful dataflow.
 - Case analysis of the issues that arise when introducing partial
   state to a distributed, high-performance stateful dataflow processing
   system.
 - Techniques for overcoming those issues while preserving system
   correctness, performance, and scalability.
 - Micro and macro evaluations of the performance and memory impact of
   introducing partial state to an application's dataflow.

## Thesis outline

 - **Introduction/Motivation**
   
   Similar to the proposal introduction. Why dataflow is useful. Why
   existing approaches are problematic.
 - **Background: The Dataflow Model**
   
   How dataflow works in general. Inputs, operators, signed updates,
   propagation, etc.
 - **Noria Overview**
   
   Important Noria-specific dataflow mechanisms. Materializations,
   dynamic dataflow, SQL-to-dataflow, thread domains, no coordiation.
   An analysis of consistency issues that can crop up in Noria.
 - **Related Work**
   
   Existing dataflow systems. Differential Dataflow arrangements and
   cyclic evictions. Windowing. Database view materialization. Maybe
   cache eviction work.
 - **The Partially Stateful Dataflow Model**
   - **Model goals**
     
     What do we want from partial state? Memory use, eviction,
     performance, consistency.
   - **Upqueries**
     
     How upqueries work conceptually. Why they present an attractive
     design (can re-use existing dataflow "forward" logic). Terminology.
   - **Invariants**
     
     Establish dataflow invariants needed to ensure that upqueries
     produce correct results.
   - **Challenges**
     
     Detail the challenges from "Technical Approach" above.
 - **Making Partial State Practical**
   
   Introduce the mechanisms outlined in the solution keys for the
   challenges in "Technical Approach". Detail what problem they solve,
   their impact, and how they related back to the invariants.
 - **Evaluation**
   - **Micro-Benchmarks**
     
     Primarily focused on benchmarking specific dataflow patterns as
     outlined in the evaluation section of the proposal.
   - **Unsharded Lobsters**
     
     The "overall application impact" benchmark outlined in the
     evaluation section.
   - **Sharded Lobsters**
     
     This is the same application as above, but run with Noria sharding
     enabled. My theory at this time is that this will work _correctly_,
     but will suffer from certain known performance problems. I have
     ideas for how these _might_ be solved, which brings us to:
 - **Future Work**
   
   Efficient upqueries across multi-level sharded shuffles. Upquery key
   subsumption: possibly no need to upquery for (A, B) if you previously
   upqueried for A. "Relaxed" aggregations that assume monotonicity of
   inputs (e.g., top-k that never upqueries). Ranged upqueries and
   upqueries across range partitioned shards.

## Proposed Timeline

 - **May 1st, 2020**: (already completed) Finish Noria implementation.
 - **June 1st, 2020**: Complete chapters on Noria, partial invariants, challenges, and solutions.
 - **July 1st, 2020**: All micro evaluations completed and written. Macro evaluations planned.
 - **August 1st, 2020**: All evaluations completed and written. Related work finished.

[osdi]: https://www.usenix.org/conference/osdi18/presentation/gjengset
