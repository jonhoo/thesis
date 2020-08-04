# Background: Noria

<!--
phew, this chapter is going to be a mouthful.

the idea here is to give enough background about how Noria works to
understand the implementation of partial, and the solutions. this is
going to have to go decently deep, since joins across upqueries for
example rely on join input materializations existing in the same domain.

things that need to be covered:

 - Noria's relation to this thesis
 - basic API
 - MIR
 - dataflow implementation and thread domains
 - migrations
 - materialization choices + indexing
   - join input materialization
   - readers (evmap)?
 - sharders

-->
