# The Partially Stateful Dataflow Model

This chapter introduces the key contribution of this thesis: the
partially stateful dataflow model. In particular, I present it here as
an extension to the model described in the previous chapter. In the next
chapter, I discuss the practical challenges in implementing partial
state. I discuss applications to other dataflow models later in the
thesis.

<!--
The challenges aren't really just about implementing "on top of Noria".
They are relatively fundamental to the approach. Should they be covered
here, or is there a clean line that can be drawn between what is
discussed here and those challenges?
-->

## Model Goals

Before I describe the proposed model for partial state, I want to take
some time to outline what properties we want from such a model. This
helps inform design decision later on, and gives a better mental picture
of the problem we are trying to tackle.

Our baseline is a system in which a stateful operator must seek and
maintain **all** the source data in order to perform **any**
computation. To give a more concrete example, an operator that performs
an aggregation grouped by some key column `g` is either
_unmaterialized_, in which case it has no state and cannot produce any
outputs, or it is _materialized_, in which case it has state for all
values of `g` and can produce any output.

We want the model to allow stateful operators to selectively pull only
data that they need, when they need it. Or, phrased differently, we want
to remove this all-or-nothing restriction. We want operators to be
_partially materialized_. If the example operator above has never been
asked to produce the value for `g = 1`, then the inputs corresponding to
`g = 1` should not need to be materialized. You can think of this as the
materializations being demand-driven, or lazy, rather than eagerly
populated. If a materialization does not have an entry for some key,
like `g = 1`, we say that `g = 1` is _missing_.

We want the model to avoid unnecessary computation, not just unnecessary
materialization. If new data arrives with `g = 1`, and `g = 1` is
missing at some operator, the model should allow discarding that data.
If it did not, the materialization would grow in response to any new
data, and would effectively be equivalent to full materialization.

Finally, we want the model's mechanism for seeding missing data to be
efficient. It should not pull data that is not explicitly needed for the
operator to make progress, and it should not require that other,
unrelated parts of the dataflow be involved in the process.

With these goals in mind, let us now dive into the partially stateful
dataflow model.

## The Core Idea: Upqueries

The core idea of this thesis work is the _upquery_. It is so named
because it is a query that flows "up" the dataflow graph, in the
opposite direction of normal dataflow messages. An upquery is a request
for a dataflow operator to **re-send certain output it sent in the
past**. This allows a downstream operator to re-construct state that it
needs but is missing. Crucially, it does so in a way that relies mostly
on the existing dataflow machinery; as we will see, most operators need
only minor changes to support upqueries. A response to an upquery is
treated (almost) exactly like a normal dataflow message; it follows the
edges of the dataflow graph, and is processed normally by operators
along the way until it reaches the querying operator.

Every upquery is for a particular key, or set of keys, that the
requesting operator needs the data for in order to continue processing.
Upqueries can easily be batched by adding to the key set. When an
upquery arrives at a materialization at an edge, the data matching the
requested keys is collected, and then forwarded as a single dataflow
message along the normal dataflow.

Upqueries rely on a key property of the dataflow: _key provenance_. If
a stateful operator performs lookups into the state `s` of an edge on
column `c`, `s` can only be partial if `c` can be traced back to a
column `c'` on some ancestor state of `s`, `s'`. Phrased differently,
the provenance of `s[c]` is known, and can be traced back to `s'[c']`.
If this provenance does not exist, then an operator that needs the state
for some `s[c] = k` cannot efficiently perform a query against `s'`. Its
only option is to ask for **all** the data in `s'`, in which case it
might as well populate `s` in its entirety.

Upqueries always ultimately originate from the application that runs the
dataflow. If new data arrives in the dataflow, and an operator
encounters missing state while processing that data, it does **not**
perform an upquery, but instead drops that data. It does this with with
the knowledge that if the data is later needed, it will be upqueried for
at that time. If an operator encounters missing state while processing
the response to an upquery on the other hand, it **does** issue an
upquery to request that data. It must do this to satisfy the upquery
that the application is waiting for.

Upqueries can recurse for other reasons too. For example, when an
upquery arrives for some state `s`, `s` may **also** be missing the
needed data, and must itself issue an upquery further up the dataflow.
Only when that recursive upquery completes can the original upquery be
satisfied.

## Key Invariants

<!--
Intro to this section.
-->

### 1. Update completeness.

> If data stops flowing into the dataflow, the dataflow will eventually
> quiesce. When it does, for every key in every state, the value for
> that key is either missing, or it reflects the effects of each input
> to the system applied exactly once. An upquery of any missing key in
> any materialization also produces every input exactly once.

The intuition here is that we want the system to _at least_ eventually
do the right thing. That is, we want to make sure that all the data
the application inserts into the dataflow is considered, that none of it
is double-counted, and that no other spurious data is added. Unless, of
course, the application has inserted dataflow operators that
double-count, in which case they should be exactly double-counted. We
also permit state to be explicitly missing to allow for partial state.

The last sentence is needed to ensure that the system _can_ also
correctly populate any missing state.

### 2. No partial updates.

> Data that encounters missing state during processing is discarded.

This invariant is key to maintaining invariant 1. Without this
invariant, an operator might receive data following an eviction, and
process that data as though the current state is _empty_, not just
missing. Consider a counting operator that did this. If the current
count of 42 for some `g = 1` is evicted, and then more data arrives with
`g = 1`, the count operator cannot assume that the current count is 0
(because it is not). It _could_ issue an upquery for the missing state,
but that would eagerly fill the partial state, which we want to avoid.

### 3. Upquery responses are snapshots.

> An upquery response for key `k` on some edge `e` must reflect all
> past messages for `k` that would have been sent on `e`, and no
> subsequent messages for `k` on `e`.

Consider what would happen if this were not the case: imagine that some
operator `o` is missing the state for `k = 1`, and so upqueries for it.
The source of the upquery, `s`, processes the upquery, and sends a
response that encapsulates messages `[m1, m2, m3]` (all with `k = 1`)
toward `o`. If `s` then sent, say, `m3` towards `o` at some later time.
`m3` would be applied twice at `o`: once when the upquery response is
processed, and again when the subsequent `m3` is processed.

Conversely, consider the case where `s` has sent some message `m` where
`m.k = 1` in the past. Since the state for `k = 1` was missing at `o` at
the time, `m` was discarded. If the subsequent upquery response from `s`
does _not_ reflect `m`, `m` is effectively lost, which would violate
invariant 1.

Note that I say "encapsulate" and "reflect" above rather than "include",
because dataflow messages are signed. `m3` might be a negative for `m1`,
in which case the upquery response from `s` for `k = 1` only
**includes** `m2`, but **encapsulates** and **reflects** `m1` through
`m3`.
<!--
I discuss why it is acceptable to collapse messages in upqueries
in this way in XXX
-->

### 4. Prefix presence.

> If state is present for some key `k` on edge `e`, then either `k` is
> not missing in any materialization above `e`, or an eviction message
> for `k` is in-flight towards `e`.

Imagine if this were not the case, and new data arrives with key `k`
along a path that eventually reaches `e`. When that data is processed by
the ancestor of `e` that is missing `k`, it is discarded following
invariant 2. As a result, the state for `k` in the materialization at
`e` is never updated to reflect the new data, which violates invariant
1. We permit upstream state to be missing as long as an eviction notice
for `k` is in-transit towards `e`, since `k` will then eventually be
missing in `e`, satisfying invariant 1.

## Challenges

Upqueries are conceptually simple. However, once you add them to any
real dataflow system, challenges arise in trying to maintain the
invariants above. I outline the main challenges below, and give
solutions to them in the next chapter.



<!--
Challenges:
 - consistency
   - exactly once
 - no blocking
 - sharding
 - multiple indices
Solutions:
 - union buffering
 - joins alternation
 - tagged paths
-->
