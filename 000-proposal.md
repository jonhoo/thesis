# Thesis Proposal

The dataflow architecture has seen a resurgence in recent years, with
systems like Kafka, Spark, Flink, and Naiad all seeing interest from
academia and industry alike. The term "dataflow" covers a wide range of
systems: streaming systems (like Kafka), stream processing systems (like
Spark and Flink), and dataflow computation systems (like Naiad). The
lines between these systems are blurry at best, as are the labels I have
put on them above. They vary, often significantly, in their design
goals, their intended uses, and their system architecture. Yet they all
share the high-level property that they take data as _input_, and feed
processed data forward through a graph of _operators_. In other words,
they have _data flow_ through operators.

This high-level design is attractive for reactive applications, where
computations are relatively fixed, but the data is changing, since it
clearly designates the data dependencies of the application. Explicitly
modeling the compute and data flow this way also allows dataflow systems
to easily scale to multiple cores or physical hosts; any dataflow edge
can be realized as a function call, a thread synchronization point, or a
network channel, and the model still "works".

These systems all face a fundamental decision when it comes to
_stateful_ operators — operators that must maintain some state in order
to process their inputs. To compute the result of a join for example,
the values in the "other side" of the join must be available to the
operator that performs the join. Traditionally, dataflow systems have
made one of three decisions in this space: ignore such operators, make
all the state the operator needs available, or keep only a subset of the
state. The third option, often called, windowing, is a popular option
because it lets the system support stateful operators without keeping
vast amounts of state at the ready just in case the operator needs it.
The window will usually keep only recent data, so that the results of
the computation is only "missing" data that is old anyway.

None of these three options are particularly attractive. Leaving the
management of state to individual operators makes it difficult to
provide consistency and fault-tolerance. Keeping all the state around
forever requires either large amounts of wasted memory or using slower,
disk-based storage to back the state. This is particularly a problem
when the application's working set is significantly smaller than its
total dataset size — resources must be provisioned for computing over
all the application's data, even though only a small subset of the
computation output is observed. Windowing works well in settings where
seeing only a subset of the data is acceptable, such as analytics, but
cannot be used in applications where complete results are needed.

I propose in this thesis to design and implement _partially-stateful
dataflow_. In a dataflow system with support for partial state,
operators act as though they have access to the full state of their
inputs. In reality, that state is lazily constructed behind the scenes;
a given piece of the input state for an operator is only produced and
stored when the operator asks for that piece. If an operator only
accesses part of its input state, the remaining parts are not computed
or stored.

This approach provides a number of benefits. First, its memory use is
**proportional to the working set** of the application, rather than to
the size of the data. Second, it works for applications that **cannot
use windowing**. Third, it allows the system to **eagerly discard**, and
avoid computation for, data that later operators have never needed, as
long as that data can later be re-produced. And finally, it allows the
application to **selectively evict** from stateful operators as the
working set changes.

The essence of the design is to introduce a notion of "missing state" to
the dataflow engine. And alongside it, a mechanism to retroactively, and
efficiently, compute over past inputs to repopulate that missing state
on demand. This design, while alluring, introduces a number of
challenges in practice. Many of these stem from operators that may now
need to communicate with their inputs to populate needed state. Those
inputs may again need to retrieve that state from their inputs, and so
on until the source of the needed data is reached. These queries for
past state flow in the "opposite" direction of the data, something
existing dataflow systems do not generally allow. When these queries are
eventually answered, the system must reconcile those results with any
data that flowed through the system while the query was pending. My
thesis will cover the specific problems that arise in depth, and provide
solutions to those problems.

## Completed Work: Noria

In OSDI 2018, I co-authored [Noria][osdi]; an implementation of the
partially stateful dataflow model for incremental view maintenance in
databases. The paper focused on building a better database backend for
read-heavy applications. A long-running dataflow program maintains any
number of materialized user-defined views, specified in SQL. We then use
joint query optimization techniques to find ways to integrate new views
and queries with the running dataflow. Noria is also highly concurrent
and distributed, and supports sharding cliques of operators to share
resource costs and increase sustainable throughput.

As I mentioned previously, dataflow is a broad term, so I want to take a
moment to discuss Noria's specific dataflow implementation. Noria takes
SQL queries from the application, and folds them into a single, unified
dataflow program. The dataflow is a directed, acyclic graph, with the
base tables at the "top", and application-accessible views at the
"bottom". The nodes in between represent the SQL operators that make up
the query of every view. Reads (`SELECT` queries) access
materializations at the leaves only, while writes (`INSERT`, `UPDATE`,
and `DELETE` queries) flow into the graph at the roots.

After a write has been persisted by the base table operator, it flows
into the dataflow graph as an "update", following the graph's edges.
Every operator the update passes through processes the update according
to the operator semantics, and emits a derived update. Eventually the
update arrives at a leaf view and changes the state visible to reads
through the leaf's materialization. Updates are _signed_ (i.e., they can
be "negative" or "positive"), with negative updates reflecting
revocations of past results, and modifications represented as a
negative-positive update pair.

My primary work on the paper revolved around the implementation of
partial state, including much of the core dataflow fabric.

Partial state was key to Noria's feasibility — without it, all results
for every prepared query must be computed and maintained, at a
significant memory and compute cost. With partial state, query results
are instead populated on demand, and only parts of the results relevant
to the application's particular query parameters are computed and
maintained. Partial state also enabled Noria to implement eviction, so
that the materialization cost is kept low even as the underlying
workload changes.

The paper introduced the "upquery" — a key piece of the partially
stateful dataflow model. An upquery reaches through the dataflow towards
its inputs, and triggers re-processing of dataflow messages needed to
compute missing state. In Noria, we observed that by modeling
re-population of state this way, most of the dataflow can remain
oblivious to the fact that state can be missing. Operators implement
only regular, "forward" data-flow, and the dataflow fabric takes care of
issuing upqueries and re-executing operators over old data as needed.

Upqueries also pose problems, however. When upqueries race with other
upqueries, or with concurrent modifications propagating through the
dataflow, the dataflow must be careful to not let permanent
inconsistencies arise. Furthermore, the system must discard data
destined for missing state early to avoid eagerly populating that
missing state. In the paper, we provide some invariants that the
partially stateful dataflow must enforce, and outline some issues
briefly by example, but leave out much of the discussion of the
underlying principles and the details of the solutions.

## Proposed Work: Partially Stateful Dataflow

In my thesis, I propose to give a more complete architecture for
partially stateful dataflow. In brief, this includes:

 - a more exhaustive analysis of the primary difficulties in realizing
   partial state in dataflow
 - comprehensive discussion of the solutions to those difficulties
 - an evaluation of the partially stateful model as implemented in Noria

In the following, I go into more detail of what that work will entail.

### Technical Approach

The thesis will build on the work from the 2018 OSDI paper, but go into
much more detail specifically about the partially stateful model. I will
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

The intuition here is that we want the system to _at least_ eventually
do the right thing. That is, we want to make sure that all the data
the application inserts into the dataflow is considered, that none of it
is double-counted, and that no other spurious data is added. Unless, of
course, the application has inserted dataflow operators that
double-count, in which case they should be exactly double-counted. We
also permit state to be explicitly missing to allow for partial state.

Several situations arise in a real dataflow implementation that
make this property difficult to uphold. I sketch the primary ones below:

#### Partial Eligibility

Partial state only makes sense if populating a particular subset of the
state of an operator is cheaper than populating its entire state. As a
trivial example consider an aggregation that counts the total number of
books in a database. If the application performs a lookup in this
aggregation's state, that lookup has no parameters. As a result, if the
state was partial, and missing, the system's only option is to query for
**all** the books, count them, and then store that count. At that point,
the state is fully materialized. Partial state did not buy us much here
beyond laziness, since the state is only ever empty or full. In a case
like this, it may even be faster for the system to eagerly materialize
the aggregation's state to avoid the overhead associated with an
asynchronous upquery.

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

#### Multi-Ancestor Operators

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
that we must determine the algorithm for upqueries across each
multi-ancestor operator separately.

#### Dependent Upqueries

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

#### Indirect Dependencies

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

#### Sharded Upqueries

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
the steady-state read performance when partial state is disabled.

Fourth, **write performance impact**. These experiments will evaluate
the change in write performance as a result of introducing partial
state. The results here will likely vary highly depending on the
specific dataflow and workload in question. In skewed dataflow programs
with fairly simple data dependencies, write performance should increase,
as unpopular keys no longer need to be maintained. In dataflow programs
with a large working set relative to the data set, performance is likely
to be mostly the same. For applications that have complex data
dependencies, the bookkeeping needed to ensure that the partial state
remains consistent may introduce a write performance penalty.

And fifth, **overall application impact**. For a complex application
with a wide range of queries and complex access patterns, it may be hard
to predict how partial state will affect the application's performance
and memory use. To estimate this, an end-to-end experiment with a
realistic application and workload is needed.

### Feasibility

The 2018 OSDI paper demonstrated that it is feasible to have partial
state in a practical dataflow system. Work I have done since then has
expanded that implementation to support more complex application
deployments, such as a sharded version of the Lobsters application from
the paper. I already have tested solutions in mind for the challenges
above, and key invariants to go alongside them. Some of the experiments
outlined I have already run as a part of the ongoing work since the 2018
OSDI paper.

### Contributions

The contributions of my thesis, subject to this proposal, will be:

 - An improved version of Noria with support for partial state even for
   complex, sharded applications.
 - Key correctness invariants for partially stateful dataflow.
 - Case analysis of the issues that arise when introducing partial
   state to a distributed, high-performance stateful dataflow processing
   system.
 - Techniques for overcoming those issues while preserving system
   correctness, performance, and scalability.
 - Micro and macro evaluations of the performance and memory impact of
   introducing partial state to an application's dataflow.

## Proposed Timeline

 - **May 1st, 2020**: (already completed) Finish Noria implementation.
 - **June 1st, 2020**: Complete chapters on Noria, partial invariants, challenges, and solutions.
 - **July 1st, 2020**: All micro evaluations completed and written. Macro evaluations planned.
 - **August 1st, 2020**: All evaluations completed and written. Related work finished.

[osdi]: https://www.usenix.org/conference/osdi18/presentation/gjengset
