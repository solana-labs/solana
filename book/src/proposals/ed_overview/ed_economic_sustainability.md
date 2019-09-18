# Economic Sustainability

Long term economic sustainability is one of the guiding principles of Solana’s economic design. While it is impossible to predict how decentralized economies will develop over time, especially economies with flexible decentralized governances, we can arrange economic components such that, under certain conditions, a sustainable economy may take shape in the long term. In the case of Solana’s network, these components take the form of token issuance \(via inflation\) and token burning’.

The dominant remittances from the Solana mining pool are validator and replicator rewards. The disinflationary mechanism is a flat, protocol-specified and adjusted, % of each transaction fee.

The Replicator rewards are to be delivered to replicators as a portion of the network inflation after successful PoRep validation. The per-PoRep reward amount is determined as a function of the total network storage redundancy at the time of the PoRep validation and the network goal redundancy. This function is likely to take the form of a discount from a base reward to be delivered when the network has achieved and maintained its goal redundancy. An example of such a reward function is shown in **Figure 3**

**Figure 3**: Example PoRep reward design as a function of global network storage redundancy.

In the example shown in Figure 1, multiple per PoRep base rewards are explored \(as a % of Tx Fee\) to be delivered when the global ledger replication redundancy meets 10X. When the global ledger replication redundancy is less than 10X, the base reward is discounted as a function of the square of the ratio of the actual ledger replication redundancy to the goal redundancy \(i.e. 10X\).

