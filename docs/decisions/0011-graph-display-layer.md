# 0011: The drawn graph is a readable view of the trace, never a different trace

**Status:** Accepted (2026-11-26)

## Context
Real traces get large: the 2009 Hal Finney payment traced 15 hops gives 270 addresses and
296 transfers; a Silk Road wallet traced back 6 hops has 54 sources feeding one address; a
40-hop peel chain is 125 addresses long. Drawing every element made graphs unreadable:
an audit of 12 real traces found up to 76 overlapping labels, 60 labels on arrowheads and
fit-to-screen zooms of 0.15–0.19 (text at 2 px). The owner reported "overlaps on the
arrows and hard to read trace graphs".

## Decision
The report's graph (`ct_trace::graph`) stays complete. The UI draws a *display graph* built
from it by pure, unit-tested functions (`app/ui/src/lib/graph.ts`, `buildDisplay`):

- **Hide small flows.** Large traces (> 40 addresses) start with flows under a share of the
  traced amount hidden: the smallest of 1 %, 2 %, 3 %, 5 %, 7.5 %, 10 % that leaves about
  40 drawn nodes. Small traces hide nothing. A notice always states how many addresses are
  hidden and under what share; a slider changes it.
- **Group** more than 4 leaf branches of one kind under one parent into one node
  ("N more: …"), keeping the 2 largest visible. **Collapse** runs of 3+ plain pass-through
  addresses into one pill ("N addresses in a row"). Both open on click.
- **Combine** transfers between the same two drawn nodes into one edge with their total
  ("×N"); its details list every transfer.
- **Never hide what is being looked at, and do not reshuffle:** selecting a lead reveals
  its main route (at each step the largest transfer on its path) whatever the settings, and
  the route stays revealed. Its addresses are never hidden or grouped; plain pass-through
  runs on it may stay collapsed (highlighted). Tiny side transfers of the same transactions
  are not added. The graph is laid out again only when the set of drawn nodes changes;
  edge and label changes are applied in place. The leads list and exports always contain every lead.
- **Readable by default:** open at a zoom where labels are about 10 px (fonts 13/12 px, zoom
  ≥ 0.8). If the route does not fit, show as much of it as fits with the lead on screen.
  "Fit" gives an overview (labels too small to read are then left out). Small flows in busy
  graphs have no permanent label (hover shows it); amounts on the selected route always show.
- **No piled-up arrows or amounts, no lines through labels:** arrows sit mid-edge (never
  under a node's label). Where 3+ edges meet at one node they are drawn as a comb (one
  trunk, a horizontal stub per branch) with each amount on its own stub. Any other edge
  that would cross a node or a label is bent around it. When nodes are added or removed
  the layout is recomputed and nodes glide to their new places (no jump; none with
  reduced motion).
- **All-outputs traces start from the transaction.** The report lists the traced outputs
  (`seed_outputs`); when they go to several addresses, the graph's start point is the
  transaction itself, with an exact-amount edge to each followed output, so outputs that are
  endpoints straight away are connected and shown.

## Consequences
- Accuracy is unaffected: hiding and grouping change only what is drawn, the notice says so,
  and amounts on grouped or combined elements are exact totals.
- Tests assert, on recorded real traces, that every drawn edge connects drawn nodes, that
  every report node is drawn, grouped or counted as hidden, that the default drawing stays
  under 45 nodes, that every lead's main route is drawn when the lead is selected, that
  revealing a route adds no other small transfers, and that all-outputs traces have one
  root and draw their largest leads.
- The display was reviewed by driving real traces in a browser (Playwright), measuring
  label overlaps and node movement, and by an independent reviewer pass.
- Thresholds are named constants in `graph.ts`; change them with this ADR.
