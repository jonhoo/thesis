# The Partially Stateful Dataflow Model

This chapter introduces the key contribution of this thesis: the
partially stateful dataflow model. In particular, I present it here as
an extension to the model described in the previous chapter. In the next
chapter, I discuss the challenges in implementing partial state on top
of Noria. I discuss applications to other dataflow models later in the
thesis.

<!--
The challenges aren't really just about implementing "on top of Noria".
They are relatively fundamental to the approach. Should they be covered
here, or is there a clean line that can be drawn between what is
discussed here and those challenges?

Challenges:
 - union buffering
 - joins alternation
 - multiple indices
 - sharding
-->

## Desired Properties

<!--
consistency
exactly once
no blocking
early discard
directed responses
-->

## Core Concepts

<!--
partial state / "holes" / "missing"
keys + key provenance
upqueries (maybe mention first, then do ^, then get back to it?)
-->

## Key Invariants

<!--
must be present upstream or about to be evicted
eventually, all updates reach non-empty state (no spurious or dupes)
one response per edge per query
-->
