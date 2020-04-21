# The Underlying Dataflow Model

This chapter outlines the particular dataflow model that this thesis
assumes. The next chapter then goes on to describe the necessary
extensions to that model to enable partial state.

Given the wide variety of systems and system designs that self-identify
as "dataflow systems", I want to take some time to describe the dataflow
model that this thesis assumes. This is the same dataflow model as the
one Noria uses, though it also shares similarities with the model used
by Naiad and Differential Dataflow.

In the Noria model, the dataflow is a connected, acyclic, directional
graph of _operators_. Every operator has some number of parents
(incoming edges), and some number of children (outgoing edges). The
edges between operators indicate data dependencies. For example, a join
operator depends on the operators that make up the sides of the join, an
aggregation depends on the operator that produces the data to be
aggregated, and a union depends on the operators whose outputs it is
taking the union of. Some operators have no incoming edges; they are the
_inputs_ to the dataflow, and are where new data initially arrives into
the system.

## Inputs and Outputs

When new input data arrives to the system at some input, that data is
forwarded along every outgoing edge from that input to its children.
Each of those children is an operator, and what each one does with the
new data it receives is up to the child's operator semantics. A filter
operator may drop the new data, while a projection operator may modify
the data before forwarding it to its children again.

There are no outgoing edges that leave the dataflow. Instead, some
operators (usually at the leaves) have side-effects that are visible
elsewhere. For example, in Noria, certain leaf operators hold shared
handles to synchronized maps that external consumers can also access.
These leaf operators update the map in response to data they receive in
the dataflow, which makes that data available to the user's application.

## Keeping State

Some operators, like aggregations, are stateful, and must be able to
persist information across dataflow messages to be useful. It would be
difficult for a count operator to tell you the current number of tulips
if all it knew was that four tulip have just been added. To support
this, an operator can request that the state of a particular dataflow
edge be _materialized_. A materialized edge can be accessed by the
operators on either end of that edge, and contains the accumulated state
across all messages ever sent along that edge. In the case of the tulip
counter, it will request that its output edge be materialized when it is
created. When it later receives notification of new tulips, it performs
a lookup into that materialization to look for the last count it sent,
adds to that count, and sends the updated count to its children.

Implemented naively, this materialization would grow without bound,
and contain every tulip count there has ever been. But for most
applications, this is wasteful, as only the _current_ state is of
interest. For this reason, the dataflow messages contain _signed_ data.
A piece of data can either be _positive_ to indicate insertion, or
_negative_ to indicate deletion. In the case of our tulip counter, when
the number of tulips changes, it emits a negative entry for the old
count, and a positive entry for the new count. When this pair of
negative and positive are applied to the materialization of the
counter's outgoing edge, the net result is that the single tulip count
is updated in-place, and the state does not grow over time.

<!-- the below is possibly more implementation than design? -->

## Concurrent Processing

In the Noria dataflow model, each operator is single-threaded, but
different operators may execute in parallel across multiple cores or
multiple computers. Adjacent operators may be grouped into a single
_thread domain_, which is then scheduled as a single unit; only one
thread can execute any of the operators in the thread domain at any one
time. Thread domains allow operators to share materializations, and
provide process-to-completion within the boundaries of the thread domain
to reduce unnecessary context switching.

Noria does not apply any coordination to its dataflow operators; all
the operators execute concurrently and independently. Any number of data
inputs may be percolating through the dataflow at any given point in
time, and they are not sequenced or otherwise tagged with timestamps.
Each dataflow edge provides in-order delivery, but operators may process
data from their incoming edges in any order. Noria requires operators to
be deterministic and commutative over their inputs to ensure that the
dataflow computation eventually converges.
