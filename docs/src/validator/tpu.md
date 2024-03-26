---
title: Transaction Processing Unit in a Solana Validator
sidebar_position: 2
sidebar_label: TPU
pagination_label: Validator's Transaction Processing Unit (TPU)
---

TPU (Transaction Processing Unit) is the logic of the validator
responsible for block production.

![TPU Block Diagram](/img/tpu.svg)

Transactions are encoded and sent in QUIC streams into the validator
from clients (other validators/users of the network) as follows:

* The quic streamer: allocates packet memory and reads the packet data from
the QUIC endpoint and applies some coalescing of packets received at
the same time. Each stream is used to transmit a packet. And there is limit on the
maximum of QUIC connections can be concurrently established between a client
identified by (IP Address, Node Pubkey) and the server. And there is a limit on the
maximum streams can be concurrently opened per connection based on the sender's
stake. Clients with higher stakes will be allowed to open more streams within
a maximum limit. The system also does rate limiting on the packets per
second(PPS) and applied the limit to the connection based on the stake.
Higher stakes offers better bandwidth. If the transfer rate is exceeded,
the server can drop the stream with the error code (15 -- STREAM_STOP_CODE_THROTTLING).
The client is expected to do some sort of exponential back off in retrying the
transactionswhen running into this situation.

* sigverify stage: deduplicates packets and applies some load-shedding
to remove excessive packets before then filtering packets with invalid
signatures by setting the packet's discard flag.

* banking stage: decides whether to forward, hold or process packets
received. Once it detects the node is the block producer it processes
held packets and newly received packets with a Bank at the tip slot.

* broadcast stage: receives the valid transactions formed into Entry's from
banking stage and packages them into shreds to send to network peers through
the turbine tree structure. Serializes, signs, and generates erasure codes
before sending the packets to the appropriate network peer.
