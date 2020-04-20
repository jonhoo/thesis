# Partially-Stateful Dataflow

The term "dataflow" covers a wide range of systems: streaming systems
like Kafka, stream processing systems like Spark and Flink, and dataflow
computation systems like Naiad. The lines between these systems are
blurry at best, as are the labels I have put on them above. They vary,
often significantly, in their design goals, their intended uses, and
their system architecture. Yet they all share the high-level property
that they take data as _input_, and feed processed data forward through
a graph of _operators_. In other words, they have _data flow_ through
operators.

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
provide consistency and fault-tolerance. Keep all the state around
forever requires either large amounts of wasted memory or using slower,
disk-based storage to back the state. Windowing works well in settings
where seeing only a subset of the data is acceptable, such as analytics,
but cannot be used in applications where complete results are needed.

In this thesis, I propose a design for, and implementation of,
_partially-stateful_ dataflow operators. In a dataflow system with
support for partial state, operators act as though they have access to
the full state of their inputs. In reality, that state is lazily
constructed behind the scenes; a given piece of the input state for an
operator is only produced and stored when the operator asks for that
piece. If an operator only accesses part of its input state, the
remaining parts are not computed or stored.

This approach provides a number of benefits. First, its memory use is
**proportional to the working set** of the application, rather than to
the size of the data. Second, it works for applications that **cannot
use windowing**. Third, it allows the system to **eagerly discard**, and
avoid computation for, data that later operators have never needed, as
long as that data can later be re-produced. And finally, it allows the
application to **selectively evict** from stateful operators as the
working set changes.

Since partial state limits extraneous memory use and unnecessary
computation, it enables applications to introduce more stateful
operators than were previously feasible, which in turn enables new,
interesting use-cases for dataflow. To demonstrate this, I built support
for the partially stateful dataflow model in the dataflow-based
materialized view database Noria. Noria includes support for a subset of
SQL, including joins, unions, aggregations, projections, and filters,
and can run in a distributed fashion across many core and physical
hosts. Results on a simulated real-world application benchmark indicate
significant improvements in both memory use and write performance, with
limited impact on the steady-state performance of materialized view
accesses.

A number of challenges arise when trying to make the partially stateful
model practical. Many of these stem from operators that may now need to
communicate with their inputs to populate needed state. Those inputs may
again need to retrieve that state from their inputs, and so on until the
source of the needed data is reached. These queries for past state flow
in the "opposite" direction of the data, something existing dataflow
systems do not generally allow. When these queries are eventually
answered, the system must then reconcile those results with any data
that flowed through the system while the query was pending. In this
thesis, I present solutions to several of these challenges in the
context of Noria. I also outline additional optimizations for larger
deployments, and other interesting use-cases that are tenable with
partially-stateful dataflow.
