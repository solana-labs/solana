# Raptor coding

## Summary
This design covers the usage of RaptorQ coding in the data plane to combat packet loss.
Every transmitted coding blob in a slot will carry the three new fields in its header.

* `start_index: u64` - index of the first data blob in this erasure set.
* `num_data: u8` - number of data blobs in this erasure set
* `num_coding: u8` - number of coding blobs in this erasure set.

The core of the change is to only transmit coding blobs. All transmitted blobs will be erasure-coded, blocktree will construct the data blobs upon receipt of a sufficient number of coded blobs.

## Window Service
Now the window service will only handle coding blobs and pass those to Blocktree.

## Blocktree
Upon receipt of a batch of coding blobs, blocktree will attempt to reconstruct the data blobs from the coding blobs. All external methods that insert data blobs directly will be removed.

Any data blobs that blocktree fails to recover are the responsibility of repair.


## Broadcast Stage
When broadcast stage is about to broadcast some new data blobs, serialize all those blobs into a single buffer. RaptorQ encode that buffer such that the resulting packets are
of size `BLOB_DATA_SIZE`. To prepare encoded blobs for transmission, serialize each packet into the data portion of a coding blob. Append to this data the serialized session information
from the encoder, as well as both the highest and lowest index of the data blobs used to generate this set of packets.

Broadcast these packets.


