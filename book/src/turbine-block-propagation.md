# Turbine Block Propagation

A Solana cluster uses a multi-layer block propagation mechanism called *Turbine*
to broadcast transaction blobs to all nodes with minimal amount of duplicate
messages.  The cluster divides itself into small collections of nodes, called
*neighborhoods*. Each node is responsible for sharing any data it receives with
the other nodes in its neighborhood, as well as propagating the data on to a
small set of nodes in other neighborhoods.  This way each node only has to
communicate with a small number of nodes.

During its slot, the leader node distributes blobs between the validator nodes
in the first neighborhood (layer 0). Each validator shares its data within its
neighborhood, but also retransmits the blobs to one node in some neighborhoods
in the next layer (layer 1). The layer-1 nodes each share their data with their 
neighborhood peers, and retransmit to nodes in the next layer, etc, until all
nodes in the cluster have received all the blobs.

## Neighborhood Assignment - Weighted Selection

In order for data plane fanout to work, the entire cluster must agree on how the
cluster is divided into neighborhoods. To achieve this, all the recognized
validator nodes (the TVU peers) are sorted by stake and stored in a list. This
list is then indexed in different ways to figure out neighborhood boundaries and
retransmit peers. For example, the leader will simply select the first nodes to
make up layer 0. These will automatically be the highest stake holders, allowing
the heaviest votes to come back to the leader first. Layer-0 and lower-layer
nodes use the same logic to find their neighbors and next layer peers.

To reduce the possibility of attack vectors, each blob is transmitted over a
random tree of neighborhoods.  Each node uses the same set of nodes representing
the cluster.  A random tree is generated from the set for each blob using
randomness derived from the blob itself.  Since the random seed is not known in
advance, attacks that try to eclipse neighborhoods from certain leaders or
blocks become very difficult, and should require almost complete control of the
stake in the cluster.

## Layer and Neighborhood Structure

The current leader makes its initial broadcasts to at most `DATA_PLANE_FANOUT`
nodes. If this layer 0 is smaller than the number of nodes in the cluster, then
the data plane fanout mechanism adds layers below. Subsequent layers follow
these constraints to determine layer-capacity: Each neighborhood contains
`DATA_PLANE_FANOUT` nodes. Layer-0 starts with 1 neighborhood with fanout nodes.
The number of nodes in each additional layer grows by a factor of fanout.

As mentioned above, each node in a layer only has to broadcast its blobs to its
neighbors and to exactly 1 node in some next-layer neighborhoods, 
instead of to every TVU peer in the cluster. A good way to think about this is, 
layer-0 starts with 1 neighborhood with fanout nodes, layer-1 adds "fanout" 
neighborhoods, each with fanout nodes and layer-2 will have 
`fanout * number of nodes in layer-1` and so on.

This way each node only has to communicate with a maximum of `2 * DATA_PLANE_FANOUT - 1` nodes.

The following diagram shows how the Leader sends blobs with a Fanout of 2 to 
Neighborhood 0 in Layer 0 and how the nodes in Neighborhood 0 share their data
with each other.

<img alt="Leader sends blobs to Neighborhood 0 in Layer 0" src="img/data-plane-seeding.svg" class="center"/>

The following diagram shows how Neighborhood 0 fans out to Neighborhoods 1 and 2.

<img alt="Neighborhood 0 Fanout to Neighborhood 1 and 2" src="img/data-plane-fanout.svg" class="center"/>

Finally, the following diagram shows a two layer cluster with a Fanout of 2.

<img alt="Two layer cluster with a Fanout of 2" src="img/data-plane.svg" class="center"/>

#### Configuration Values

`DATA_PLANE_FANOUT` - Determines the size of layer 0. Subsequent
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
