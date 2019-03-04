# Data Plane Fanout

A Solana cluster uses a multi-layer mechanism called *data plane fanout* to
broadcast transaction blobs to all nodes in a very quick and efficient manner.
In order to establish the fanout, the cluster divides itself into small
collections of nodes, called *neighborhoods*. Each node is responsible for
sharing any data it receives with the other nodes in its neighborhood, as well
as propagating the data on to a small set of nodes in other neighborhoods.

During its slot, the leader node distributes blobs between the validator nodes
in one neighborhood (layer 1). Each validator shares its data within its
neighborhood, but also retransmits the blobs to one node in each of multiple
neighborhoods in the next layer (layer 2). The layer-2 nodes each share their
data with their neighborhood peers, and retransmit to nodes in the next layer,
etc, until all nodes in the cluster have received all the blobs.

<img alt="Two layer cluster" src="img/data-plane.svg" class="center"/>

## Neighborhood Assignment - Weighted Selection

In order for data plane fanout to work, the entire cluster must agree on how the
cluster is divided into neighborhoods. To achieve this, all the recognized
validator nodes (the TVU peers) are sorted by stake and stored in a list. This
list is then indexed in different ways to figure out neighborhood boundaries and
retransmit peers. For example, the leader will simply select the first nodes to
make up layer 1. These will automatically be the highest stake holders, allowing
the heaviest votes to come back to the leader first. Layer-1 and lower-layer
nodes use the same logic to find their neighbors and lower layer peers.

## Layer and Neighborhood Structure

The current leader makes its initial broadcasts to at most `DATA_PLANE_FANOUT`
nodes. If this layer 1 is smaller than the number of nodes in the cluster, then
the data plane fanout mechanism adds layers below. Subsequent layers follow
these constraints to determine layer-capacity: Each neighborhood contains
`NEIGHBORHOOD_SIZE` nodes and each layer may have up to `DATA_PLANE_FANOUT/2`
neighborhoods.

As mentioned above, each node in a layer only has to broadcast its blobs to its
neighbors and to exactly 1 node in each next-layer neighborhood, instead of to
every TVU peer in the cluster. In the default mode, each layer contains
`DATA_PLANE_FANOUT/2` neighborhoods. The retransmit mechanism also supports a
second, `grow`, mode of operation that squares the number of neighborhoods
allowed each layer. This dramatically reduces the number of layers needed to
support a large cluster, but can also have a negative impact on the network
pressure on each node in the lower layers. A good way to think of the default
mode (when `grow` is disabled) is to imagine it as chain of layers, where the
leader sends blobs to layer-1 and then layer-1 to layer-2 and so on, the `layer
capacities` remain constant, so all layers past layer-2 will have the same
number of nodes until the whole cluster is covered. When `grow` is enabled, this
becomes a traditional fanout where layer-3 will have the square of the number of
nodes in layer-2 and so on.

#### Configuration Values

`DATA_PLANE_FANOUT` - Determines the size of layer 1. Subsequent
layers have `DATA_PLANE_FANOUT/2` neighborhoods when `grow` is inactive.

`NEIGHBORHOOD_SIZE` - The number of nodes allowed in a neighborhood.
Neighborhoods will fill to capacity before new ones are added, i.e if a
neighborhood isn't full, it _must_ be the last one.

`GROW_LAYER_CAPACITY` - Whether or not retransmit should be behave like a
_traditional fanout_, i.e if each additional layer should have growing
capacities. When this mode is disabled (default), all layers after layer 1 have
the same capacity, keeping the network pressure on all nodes equal.

Currently, configuration is set when the cluster is launched. In the future,
these parameters may be hosted on-chain, allowing modification on the fly as the
cluster sizes change.

## Neighborhoods

The following diagram shows how two neighborhoods in different layers interact.
What this diagram doesn't capture is that each neighbor actually receives
blobs from one validator per neighborhood above it. This means that, to
cripple a neighborhood, enough nodes (erasure codes +1 per neighborhood) from
the layer above need to fail.  Since multiple neighborhoods exist in the upper
layer and a node will receive blobs from a node in each of those neighborhoods,
we'd need a big network failure in the upper layers to end up with incomplete
data.

<img alt="Inner workings of a neighborhood"
src="img/data-plane-neighborhood.svg" class="center"/>
