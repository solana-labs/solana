# Data Plane Fanout

A Solana cluster uses a multi-layer mechanism called *data plane fanout* to
broadcast transaction blobs to all nodes in a very quick and efficient manner.
In order to establish the fanout, the cluster divides itself into small
collections of nodes, called *neighborhoods*. Each node is responsible for
sharing any data it receives with the other nodes in its neighborhood, as well
as propagating the data on to a small set of nodes in other neighborhoods. 
This way each node only has to communicate with a small number of nodes.

During its slot, the leader node distributes blobs between the validator nodes
in one neighborhood (layer 1). Each validator shares its data within its
neighborhood, but also retransmits the blobs to one node in some neighborhoods 
in the next layer (layer 2). The layer-2 nodes each share their data with their 
neighborhood peers, and retransmit to nodes in the next layer, etc, until all 
nodes in the cluster have received all the blobs.

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
`DATA_PLANE_FANOUT` nodes. Layer-1 starts with 1 neighborhood with fanout nodes.
The number of nodes in each additional layer grows by a factor 
of fanout.

As mentioned above, each node in a layer only has to broadcast its blobs to its
neighbors and to exactly 1 node in some next-layer neighborhoods, 
instead of to every TVU peer in the cluster. A good way to think about this is, 
layer-1 starts with 1 neighborhood with fanout nodes, layer-2 adds fanout 
neighborhoods, each with fanout nodes and layer-3 will have 
`fanout * number of nodes in layer-2` and so on.

This way each node only has to communicate with a maximum of `2 * DATA_PLANE_FANOUT - 1` nodes.

#### Configuration Values

`DATA_PLANE_FANOUT` - Determines the size of layer 1. Subsequent
layers grow by a factor of `DATA_PLANE_FANOUT`. 
The number of nodes in a neighborhood is equal to the fanout value.
Neighborhoods will fill to capacity before new ones are added, i.e if a
neighborhood isn't full, it _must_ be the last one.

Currently, configuration is set when the cluster is launched. In the future,
these parameters may be hosted on-chain, allowing modification on the fly as the
cluster sizes change.

## Neighborhoods

The following diagram shows how two neighborhoods in different layers interact.
To cripple a neighborhood, enough nodes (erasure codes +1) from the neighborhood 
above need to fail. Since each neighborhood receives blobs from multiple nodes 
in a neighborhood in the upper layer, we'd need a big network failure in the upper 
layers to end up with incomplete data.

<img alt="Inner workings of a neighborhood"
src="img/data-plane-neighborhood.svg" class="center"/>
