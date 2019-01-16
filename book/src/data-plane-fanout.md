# Data Plane Fanout

This article describes the current single-layer broadcast and retransmit mechanisms as well as proposed changes to add a multi-layer retransmit via an Avalanche mechanism.

## Current Design

There's two basic parts to the current data plane's fanout design.

#### Broadcast Service

In this service, the leader distributes its data across the Layer-1 nodes. Currently, Layer-1 nodes are all known "TVU peers" (`ClusterInfo::tvu_peers`).
The leader performs a round-robin broadcast where it sends each blob of data to only one validator at a time. That way each Layer-1 node only receives partial data from the leader and
the Retransmit Stage in each Layer-1 node's TVU will ensure all data is shared between its Layer-1 peers and a complete window is received.

#### Retransmit Stage

The Retransmit stage *forwards* data from a Layer-1 node to all of _its_ "retransmit peers" (list of TVU peers excluding the leader). So as nodes start seeing complete windows they can send their votes back to the leader.
Validators know to only forward blobs that came from the leader by checking the signatures against the current leader.

**Cluster_info -> retransmit** = Used by TVUs to retransmit. Currently Layer-1 sends this to all TVU peers, leader is automatically excluded.

**Cluster_info -> broadcast** = Used by leader to broadcast to layer-1 nodes.

**BroadcastService -> run** = Used by leader (TPU) to broadcast to all validators. Currently all TVU Peers are considered layer-1. But blobs are transmitted sort of round robin. See Cluster_info->Broadcast.

## Proposed Design

The new design organizes the network by stake and divides it into a collection of nodes, called `neighborhoods`.
The leader broadcasts its blobs to the layer-1 (neighborhood 0) nodes exactly like it does without this mechanism. The main difference being the number of nodes in layer-1 is capped via the configurable `DATA_PLANE_FANOUT`. If the fanout is smaller than the nodes in the network then the mechanism will add layers below layer-1. Subsequent layers (beyond layer-1) follow the following constraints to determine layer-capacity.
Each neighborhood has `NEIGHBORHOOD_SIZE` nodes and `fanout/2` neighborhoods are allowed per layer.

Nodes in a layer will broadcast their blobs to exactly 1 node in each next-layer neighborhood and each node in a neighborhood will perform retransmits amongst its neighbors (just like layer-1 does with its layer-1 peers).
This means any node has to only send its data to its neighbors and each neighborhood in the layer below instead of every single TVU peer it has.
The retransmit mechanism also supports a second, `grow`,  mode of operation that squares the number of neighborhoods allowed per layer which dramatically reduces the number of layers needed to support a large network but can also have a negative impact on the network pressure each node in the lower layers has to deal with.
A good way to think of the default mode (when `grow` is disabled) is to imagine it as `chain` of layers where the leader sends blobs to layer-1 and then layer-1 to layer-2 and so on, but instead of growing layer-3 to the square of number of nodes in layer-2, we keep the `layer capacities` constant, so all layers past layer-2 will have the same number of nodes until the whole network is covered. When `grow` is enabled, this
quickly turns into a traditional fanout where layer-3 will have the square of the number of nodes in layer-2 and so on.

Below is an example of a two layer network. Note - this example doesn't describe the same `fanout/2` limit for lower layer neighborhoods.

<img alt="Two layer network" src="img/data-plane.svg" class="center"/>

#### Neighborhoods

The following diagram shows how two neighborhoods in different layers interact. What this diagram doesn't capture
is that each `neighbor` actually receives blobs from 1 one validator _per_ neighborhood above it. This means that, to cripple a neighborhood, enough nodes (erasure codes +1 _per_ neighborhood) from the layer above need to fail.
Since multiple neighborhoods exist in the upper layer and a node will receive blobs from a node in each of those neighborhoods, we'd need a big network failure in the upper layers to end up with incomplete data.

<img alt="Inner workings of a neighborhood" src="img/data-plane-neighborhood.svg" class="center"/>

#### A Weighted Selection Mechanism

To support this mechanism, there needs to be a agreed upon way of dividing the network amongst the nodes. To achieve this the `tvu_peers` are sorted by stake and stored in a list. This list can then be indexed in different ways to figure out neighborhood boundaries and retransmit peers.
For example, the leader will simply select the first `DATA_PLANE_FANOUT` nodes as its layer 1 nodes. These will automatically be the highest stake holders allowing the heaviest votes to come back to the leader first.
The same logic determines which nodes each node in layer needs to retransmit its blobs to. This involves finding its neighbors and lower layer peers.

#### Broadcast Service
Broadcast service uses a bank to figure out stakes and hands this off to ClusterInfo to figure out the top `DATA_PLANE_FANOUT` stake holders.
These top stake holders will be considered Layer 1. For the leader this is pretty straightforward and can be achieved with a `truncate` call on a sorted list of peers.

#### Retransmit Stage
The biggest challenge in updating to this mechanism is to update the retransmit stage and make it "layer aware"; i.e using the bank each node can figure out which layer it belongs in and which lower layer peers nodes to send blobs to with minimal overlap across neighborhood boundaries.
Overlaps will be minimized based on `((node.layer_index) % (layer_neighborhood_size) * cur_neighborhood_index)` where `cur_neighborhood_index` is the loop index in `num_neighborhoods` so that a node only forwards blobs to a single node in a lower layer neighborhood.

Each node can receive blobs froms its peer in the layer above as well as its neighbors. As long as the failure rate is less than the number of erasure codes, blobs can be repaired without the network failing.

#### Constraints

`DATA_PLANE_FANOUT` - The size of layer 1 is determined by this. Subsequent layers have `DATA_PLANE_FANOUT/2` neighborhoods when `grow` is inactive.

`NEIGHBORHOOD_SIZE` - The number of nodes allowed in a neighborhood. Neighborhoods will fill to capacity before new ones are added, i.e if a neighborhood isn't full, it _must_ be the last one.

`GROW_LAYER_CAPACITY` - Whether or not retransmit should be behave like a _traditional fanout_, i.e if each additional layer should have growing capacities. When this mode is disabled (default) all layers after layer 1 have the same capacity to keep the network pressure on all nodes equal.

Future work would involve moving these parameters to on chain configuration since it might be beneficial tune these on the fly as the network sizes change.

