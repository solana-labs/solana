# Replication via Avalanche 

This RFC describes the current single layer replication mechanism as well as proposed changes to add a second layer and implementing an Avalanche mechanism.

## Current Design
There's two basic parts to the current Replication design. 

#### Broadcast stage:
In this stage, the leader distributes its data across the Layer-1 nodes. Currently, Layer-1 nodes are all known "tvu peers" (ClusterInfo::tvu_peers). 
That way each Layer-1 node only receives partial data from the leader and the Replicate Stage in each Layer-1 node's TVU will ensure all data is shared between its Layer-1 peers 
and a complete window is received.   

The broadcast stage sends its data and "broadcast_table" (tvu peers currently) to ClusterInfo to complete the broadcast. ClusterInfo then 
sets up the broadcast "orders" by distributing the window's blobs across all broadcast_table entities. 

#### Replicate stage:  
The Replicate stage *forwards* data from a Layer-1 node to all of its "tvu peers". So once as nodes start seeing complete windows they can sending their votes back to the leader and once 2/3 + 1 majority is 
reached we can say that finality has been achieved and the leader can move on.

**Cluster_info -> retransmit** = Used by TVUs to retransmit. Currently Layer-1 sends this to all tvu peers, leader is automatically excluded. This stage has bank and should be able to use it for weighted selection in the new design.

**Cluster_info -> broadcast** = Used by leader to broadcast to layer-1 nodes. 

**BroadcastStage -> run** = Used by leader (TPU) to broadcast to all validators. Currently all TVU Peers are considered layer-1. But blobs are transmitted sort of round robin. See Cluster_info->Broadcast. This stage doesn't have bank and will need it for weighted selection.


## New Design

                                     .-------------.
                                     |             |
                       .-------------+   Leader    +══════════════╗
                       |             |             |              ║
                       |             `-------------`              ║
                       v                                          v
                .-------------.                            .-------------.
                |             +--------------------------->|             |
           .----+ Validator 1 |                            | Validator 2 +═══╗
           |    |             |<═══════════════════════════+             |   ║
           |    `------+------`                            `------+------`   ║
           |           |                                          ║          ║
           |           `------------------------------.           ║          ║
           |                                          |           ║          ║
           |                     ╔════════════════════════════════╝          ║
           |                     ║                    |                      ║
           V                     v                    V                      v
    .-------------.       .-------------.       .-------------.       .-------------.
    |             |       |             |       |             |       |             |
    | Validator 3 +------>| Validator 4 +══════>| Validator 5 +------>| Validator 6 |
    |             |       |             |       |             |       |             |
    `-------------`       `-------------`       `-------------`       `------+------`
           ^                                                                 ║
           ║                                                                 ║
           ╚═════════════════════════════════════════════════════════════════╝

#### What’s needed -
The main goal of the new design is to enable Layer-2 nodes in the replication process. There are a few challenges with this and we should be able 
to address some of them.  

* Most importantly, there needs to be a mechanism to figure out which nodes belong in what layer of this avalanche. We need a Stake weighted selection for the nodes. This is missing and is very important. 
* Leaders should only broadcast to layer 1 nodes and the network should be split (ClusterInfo can provide leader’s layer-1 tvu peers) 
Layer 1 nodes should broadcast to all peers at the same layer and to a subset of peers at lower layers. 
(ClusterInfo can provide Layer 1’s peers and layer 2 sections)
* Layer 2 nodes should only retransmit amongst themselves. They should also be sectioned off and only need to communicate with each other instead of all layer 2 nodes. 
(ClusterInfo can provide which nodes are in this node’s neighborhood)
* 200 fanout from leader to layer-1 and layer-1 to layer-2.
* Need to make sure layer-2 has a neighborhood (stake based) and that there can be some overlap. 


### The proposed changes
The idea is to fanout to 200 nodes from leader to layer 1 and then from layer 1 to layer 2. Layer 2 nodes should be semi-aware of this fanout so that they only talk to nodes in their "neighborhood". 
This will result in the formation of multiple layer-2 neighborhoods that only care about communicating with each other and then the leader to send their votes. 
Each layer 1 node only needs to broadcast its data to a single node in each neighborhood. With this in mind, the following changes will be needed to the two main stages.

#### A Stake weighted selection mechanism:
Need to come up with some logic to filter a set of nodes based on a stake range or number boundary. For the leader to find its layer 1 nodes, we need the biggest stake holders. 
For each layer 1 node to find its layer 1 peers, the same logic *should* work but each layer 1 node also needs to find its layer 2 group. This needs some thought.
Stake weights should work once we know the weight of the last layer 1 node. Layer 2 nodes can be "ordered" by weight and all layer 1 nodes should be able to avoid stepping over each others layer 2 nodes.    
 
#### Broadcast stage:
Broadcast Stage might need a bank to figure out stakes and can hand this off to ClusterInfo to figure out the top 200/85% stake holders. 
These top stake holders will be considered Layer 1. For the leader this step should be pretty straightforward and can be achieved with a `get_top_tvus()` call to sort the top stake holders' tvus. 

#### Replicate stage:  
The biggest challenge is to update replicate stage and make it "layer aware"; i.e using the bank each layer 1 node can figure out which layer 2 nodes to send blobs to with minimal overlap. 
The plan to avoid this overlap
will be to broadcast using indices based on `((node.layer_1_index) % (layer-2_neighborhood_size) * cur_neighborhood_index)` where `cur_neighborhood_index` is loop index in `num_neighborhoods`.

Layer 2 nodes should be able to reuse this logic to avoid going *up* to layer 1 and to only replicate within its neighborhood.
As long as the stake weight of the first layer 2 node is known(or the last layer 1) given that the fanout will be fixed we should be able to figure out with decent certainty who the layer 2 neighbors are.
   

We can cache most of the broadcast pool and only update this on some interval since the network shouldn't be changing drastically once it's stable. Recomputing neighborhoods and layers every broadcast stage could be 
unnecessarily expensive. 