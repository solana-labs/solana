---
title: Creating Compressed NFTs with JavaScript
description:
  "Compressed NFTs use the Bubblegum program from Metaplex to cheaply and
  securely store NFT metadata using State Compression on Solana."
keywords:
  - compression
  - merkle tree
  - read api
  - metaplex
---

Compressed NFTs on Solana use the
[Bubblegum](https://docs.metaplex.com/programs/compression/) program from
Metaplex to cheaply and securely store NFT metadata using
[State Compression](../../learn/state-compression.md).

This developer guide will use JavaScript/TypeScript to demonstrate:

- [how to create a tree for compressed NFTs](#create-a-tree),
- [how to mint compressed NFTs into a tree](#mint-compressed-nfts),
- [how to get compressed NFT metadata from the Read API](#reading-compressed-nfts-metadata),
  and
- [how to transfer compressed NFTs](#transfer-compressed-nfts)

## Intro to Compressed NFTs

Compressed NFTs use [State Compression](../../learn/state-compression.md) and
[merkle trees](../../learn/state-compression.md#what-is-a-merkle-tree) to
drastically reduce the storage cost for NFTs. Instead of storing an NFT's
metadata in a typical Solana account, compressed NFTs store the metadata within
the ledger. This allows compressed NFTs to still inherit the security and speed
of the Solana blockchain, while at the same time reducing the overall storage
costs.

Even though the on-chain data storage mechanism is different than their
uncompressed counterparts, compressed NFTs still follow the exact same
[Metadata](https://docs.metaplex.com/programs/token-metadata/accounts#metadata)
schema/structure. Allowing you to define your Collection and NFT in an identical
way.

However, the process to mint and transfer compressed NFTs is different from
uncompressed NFTs. Aside from using a different on-chain program, compressed
NFTs are minting into a merkle tree and require verification of a "proof" to
transfer. More on this below.

### Compressed NFTs and indexers

Since compressed NFTs store all of their metadata in the
[ledger](../../terminology.md#ledger), instead of in traditional
[accounts](../../terminology.md#account) like uncompressed NFTs, we will need to
help of indexing services to quickly fetch our compressed NFT's metadata.

Supporting RPC providers are using the Digital Asset Standard Read API (or "Read
API" for short) to add additional RPC methods that developers can call. These
additional, NFT oriented methods, are loaded with all the information about
particular NFTs. Including support for **BOTH** compressed NFTs **AND**
uncompressed NFTs.

:::caution Metadata is secured by the ledger and cached by indexers

Since validators do not keep a very long history of the recent ledger data,
these indexers effectively "cache" the compressed NFT metadata passed through
the Solana ledger. Quickly serving it back on request to improve speed and user
experience of applications.

However, since the metadata was already secured by the ledger when minting the
compressed NFT, anyone could re-index the metadata directly from the secure
ledger. Allowing for independent verification of the data, should the need or
desire arise.

:::

These indexing services are already available from some of the common RPC
providers, with more rolling out support in the near future. To name a few of
the RPC providers that already support the Read API:

- Helius
- Triton
- SimpleHash

### How to mint compressed NFTs

The process to create or mint compressed NFTs on Solana is similar to creating a
"traditional NFT collection", with a few differences. The mint process will
happen in 3 primary steps:

- create an NFT collection (or use an existing one)
- create a
  [concurrent merkle tree](../../learn/state-compression.md#what-is-a-concurrent-merkle-tree)
  (using the `@solana/spl-account-compression` SDK)
- mint compressed NFTs into your tree (to any owner's address you want)

### How to transfer a compressed NFT

Once your compressed NFT exists on the Solana blockchain, the process to
transfer ownership of a compressed NFT happens in a few broad steps:

1. get the NFT "asset" information (from the indexer)
2. get the NFT's "proof" (from the indexer)
3. get the Merkle tree account (from the Solana blockchain)
4. prepare the asset proof (by parsing and formatting it)
5. build and send the transfer instruction

The first three steps primarily involve gathering specific pieces of information
(the `proof` and the tree's canopy depth) for the NFT to be transferred. These
pieces of information are needed to correctly parse/format the `proof` to
actually be sent within the transfer instruction itself.

## Getting started

For this guide, we are going to make a few assumptions about the compressed NFT
collection we are going to create:

- we are going to use TypeScript and NodeJS for this example
- we will use a single, **new** Metaplex collection

### Project Setup

Before we start creating our compressed NFT collection, we need to install a few
packages:

- [`@solana/web3.js`](https://www.npmjs.com/package/@solana/web3.js) - the base
  Solana JS SDK for interacting with the blockchain, including making our RPC
  connection and sending transactions
- [`@solana/spl-token`](https://www.npmjs.com/package/@solana/spl-token) - used
  in creating our collection and mint on-chain
- [`@solana/spl-account-compression`](https://www.npmjs.com/package/@solana/spl-account-compression) -
  used to create the on-chain tree to store our compressed NFTs
- [`@metaplex-foundation/mpl-bubblegum`](https://www.npmjs.com/package/@metaplex-foundation/mpl-bubblegum) -
  used to get the types and helper functions for minting and transferring
  compressed NFTs on-chain
- [`@metaplex-foundation/mpl-token-metadata`](https://www.npmjs.com/package/@metaplex-foundation/mpl-token-metadata) -
used to get the types and helper functions for our NFT's metadata
<!-- - [`@metaplex-foundation/js`](https://www.npmjs.com/package/@metaplex-foundation/js) -->

Using your preferred package manager (e.g. npm, yarn, pnpm, etc), install these
packages into your project:

```sh
yarn add @solana/web3.js @solana/spl-token @solana/spl-account-compression
```

```sh
yarn add @metaplex-foundation/mpl-bubblegum @metaplex-foundation/mpl-token-metadata
```

## Create a Collection

NFTs are normally grouped together into a
[Collection](https://docs.metaplex.com/programs/token-metadata/certified-collections#collection-nfts)
using the Metaplex standard. This is true for **BOTH** traditional NFTs **AND**
compressed NFTs. The NFT Collection will store all the broad metadata for our
NFT grouping, such as the collection image and name that will appear in wallets
and explorers.

Under the hood, an NFT collection acts similar to any other token on Solana.
More specifically, a Collection is effectively a uncompressed NFT. So we
actually create them following the same process of creating an
[SPL token](https://spl.solana.com/token):

- create a new token "mint"
- create a associated token account (`ata`) for our token mint
- actually mint a single token
- store the collection's metadata in an Account on-chain

Since NFT Collections having nothing special to do with
[State Compression](../../learn/state-compression.md) or
[compressed NFTs](./compressed-nfts.md), we will not cover creating one in this
guide.

### Collection addresses

Even though this guide does not cover creating one, we will need the many of the
various addresses for your Collection, including:

- `collectionAuthority` - this may be your `payer` but it also might not be
- `collectionMint` - the collection's mint address
- `collectionMetadata` - the collection's metadata account
- `editionAccount` - for example, the `masterEditionAccount` created for your
  collection

## Create a tree

One of the most important decisions to make when creating compressed NFTs is
[how to setup your tree](../../learn/state-compression.md#sizing-a-concurrent-merkle-tree).
Especially since the values used to size your tree will determine the overall
cost of creation, and **CANNOT** be changed after creation.

:::caution

A tree is **NOT** the same thing as a collection. A single collection can use
_any_ number of trees. In fact, this is usually recommended for larger
collections due to smaller trees having greater composability.

Conversely, even though a tree **could** be used in multiple collections, it is
generally considered an anti-pattern and is not recommended.

:::

Using the helper functions provided by the
[`@solana/spl-account-compression`](https://www.npmjs.com/package/@solana/spl-account-compression)
SDK, we can create our tree in the following steps:

- decide on our tree size
- generate a new Keypair and allocated space for the tree on-chain
- actually create the tree (making it owned by the Bubblegum program)

### Size your tree

Your tree size is set by 3 values, each serving a very specific purpose:

1. `maxDepth` - used to determine how many NFTs we can have in the tree
2. `maxBufferSize` - used to determine how many updates to your tree are
   possible in the same block
3. `canopyDepth` - used to store a portion of the proof on chain, and as such is
   a large of cost and composability of your compressed NFT collection

:::info

Read more about the details about
[State Compression](../../learn/state-compression.md), including
[how to size a tree](../../learn/state-compression.md#sizing-a-concurrent-merkle-tree)
and potential composability concerns.

:::

Let's assume we are going to create a compressed NFT collection with 10k NFTs in
it. And since our collection is relatively small, we only need a single smaller
tree to store all the NFTs:

```ts
// define the depth and buffer size of our tree to be created
const maxDepthSizePair: ValidDepthSizePair = {
  // max=16,384 nodes (for a `maxDepth` of 14)
  maxDepth: 14,
  maxBufferSize: 64,
};

// define the canopy depth of our tree to be created
const canopyDepth = 10;
```

Setting a `maxDepth` of `14` will allow our tree to hold up to `16,384`
compressed NFTs, more than exceeding our `10k` collection size.

Since only specific
[`ValidDepthSizePair`](https://solana-labs.github.io/solana-program-library/account-compression/sdk/docs/modules/index.html#ValidDepthSizePair)
pairs are allowed, simply set the `maxBufferSize` to the corresponding value
tied to your desired `maxDepth`.

Next, setting `canopyDepth` of `10` tells our tree to store `10` of our "proof
node hashes" on-chain. Thus requiring us to always include `4` proof node values
(i.e. `maxDepth - canopyDepth`) in every compressed NFT transfer instruction.

### Generate addresses for the tree

When creating a new tree, we need to generate a new
[Keypair](../../terminology.md#keypair) address for the tree to have:

```ts
const treeKeypair = Keypair.generate();
```

Since our tree will be used for compressed NFTs, we will also need to derive an
Account with authority that is owned by the Bubblegum program (i.e. PDA):

```ts
// derive the tree's authority (PDA), owned by Bubblegum
const [treeAuthority, _bump] = PublicKey.findProgramAddressSync(
  [treeKeypair.publicKey.toBuffer()],
  BUBBLEGUM_PROGRAM_ID,
);
```

### Build the tree creation instructions

With our tree size values defined, and our addresses generated, we need to build
two related instructions:

1. allocate enough space on-chain for our tree
2. actually create the tree, owned by the Bubblegum program

Using the
[`createAllocTreeIx`](https://solana-labs.github.io/solana-program-library/account-compression/sdk/docs/modules/index.html#createAllocTreeIx)
helper function, we allocate enough space on-chain for our tree.

```ts
// allocate the tree's account on chain with the `space`
const allocTreeIx = await createAllocTreeIx(
  connection,
  treeKeypair.publicKey,
  payer.publicKey,
  maxDepthSizePair,
  canopyDepth,
);
```

Then using the
[`createCreateTreeInstruction`](https://metaplex-foundation.github.io/metaplex-program-library/docs/bubblegum/functions/createCreateTreeInstruction.html)
from the Bubblegum SDK, we actually create the tree on-chain. Making it owned by
the Bubblegum program.

```ts
// create the instruction to actually create the tree
const createTreeIx = createCreateTreeInstruction(
  {
    payer: payer.publicKey,
    treeCreator: payer.publicKey,
    treeAuthority,
    merkleTree: treeKeypair.publicKey,
    compressionProgram: SPL_ACCOUNT_COMPRESSION_PROGRAM_ID,
    // NOTE: this is used for some on chain logging
    logWrapper: SPL_NOOP_PROGRAM_ID,
  },
  {
    maxBufferSize: maxDepthSizePair.maxBufferSize,
    maxDepth: maxDepthSizePair.maxDepth,
    public: false,
  },
  BUBBLEGUM_PROGRAM_ID,
);
```

### Build and send the transaction

With our two instructions built, we can add them into a transaction and send
them to the blockchain, making sure both the `payer` and generated `treeKeypair`
sign the transaction:

```ts
// build the transaction
const tx = new Transaction().add(allocTreeIx).add(createTreeIx);
tx.feePayer = payer.publicKey;

// send the transaction
const txSignature = await sendAndConfirmTransaction(
  connection,
  tx,
  // ensuring the `treeKeypair` PDA and the `payer` are BOTH signers
  [treeKeypair, payer],
  {
    commitment: "confirmed",
    skipPreflight: true,
  },
);
```

After a few short moments, and once the transaction is confirmed, we are ready
to start minting compressed NFTs into our tree.

## Mint compressed NFTs

Since compressed NFTs follow the same Metaplex
[metadata standards](https://docs.metaplex.com/programs/token-metadata/accounts#metadata)
as traditional NFTs, we can define our actual NFTs data the same way.

The primary difference is that with compressed NFTs the metadata is actually
stored in the ledger (unlike traditional NFTs that store them in accounts). The
metadata gets "hashed" and stored in our tree, and by association, secured by
the Solana ledger.

Allowing us to cryptographically verify that our original metadata has not
changed (unless we want it to).

:::info

Learn more about how State Compression uses
[concurrent merkle trees](../../learn/state-compression.md#what-is-a-concurrent-merkle-tree)
to cryptographically secure off-chain data using the Solana ledger.

:::

### Define our NFT's metadata

We can define the specific metadata for the single NFT we are about to mint:

```ts
const compressedNFTMetadata: MetadataArgs = {
  name: "NFT Name",
  symbol: "ANY",
  // specific json metadata for each NFT
  uri: "https://supersweetcollection.notarealurl/token.json",
  creators: null,
  editionNonce: 0,
  uses: null,
  collection: null,
  primarySaleHappened: false,
  sellerFeeBasisPoints: 0,
  isMutable: false,
  // these values are taken from the Bubblegum package
  tokenProgramVersion: TokenProgramVersion.Original,
  tokenStandard: TokenStandard.NonFungible,
};
```

In this demo, the key pieces of our NFT's metadata to note are:

- `name` - this is the actual name of our NFT that will be displayed in wallets
  and on explorers.
- `uri` - this is the address for your NFTs metadata JSON file.
- `creators` - for this example, we are not storing a list of creators. If you
  want your NFTs to have royalties, you will need to store actual data here. You
  can checkout the Metaplex docs for more info on it.

### Derive the Bubblegum signer

When minting new compressed NFTs, the Bubblegum program needs a PDA to perform a
[cross-program invocation](../programming-model/calling-between-programs#cross-program-invocations)
(`cpi`) to the SPL compression program.

:::caution

This `bubblegumSigner` PDA is derived using a hard coded seed string of
`collection_cpi` and owned by the Bubblegum program. If this hard coded value is
not provided correctly, your compressed NFT minting will fail.

:::

Below, we derive this PDA using the **required** hard coded seed string of
`collection_cpi`:

```ts
// derive a PDA (owned by Bubblegum) to act as the signer of the compressed minting
const [bubblegumSigner, _bump2] = PublicKey.findProgramAddressSync(
  // `collection_cpi` is a custom prefix required by the Bubblegum program
  [Buffer.from("collection_cpi", "utf8")],
  BUBBLEGUM_PROGRAM_ID,
);
```

### Create the mint instruction

Now we should have all the information we need to actually mint our compressed
NFT.

Using the `createMintToCollectionV1Instruction` helper function provided in the
Bubblegum SDK, we can craft the instruction to actually mint our compressed NFT
directly into our collection.

If you have minted traditional NFTs on Solana, this will look fairly similar. We
are creating a new instruction, giving several of the account addresses you
might expect (e.g. the `payer`, `tokenMetadataProgram`, and various collection
addresses), and then some tree specific addresses.

The addresses to pay special attention to are:

- `leafOwner` - this will be the owner of the compressed NFT. You can either
  mint it your self (i.e. the `payer`), or airdrop to any other Solana address
- `leafDelegate` - this is the delegated authority of this specific NFT we are
  about to mint. If you do not want to have a delegated authority for the NFT we
  are about to mint, then this value should be set to the same address of
  `leafOwner`.

```ts
const compressedMintIx = createMintToCollectionV1Instruction(
  {
    payer: payer.publicKey,

    merkleTree: treeAddress,
    treeAuthority,
    treeDelegate: payer.publicKey,

    // set the receiver of the NFT
    leafOwner: receiverAddress || payer.publicKey,
    // set a delegated authority over this NFT
    leafDelegate: payer.publicKey,

    // collection details
    collectionAuthority: payer.publicKey,
    collectionAuthorityRecordPda: BUBBLEGUM_PROGRAM_ID,
    collectionMint: collectionMint,
    collectionMetadata: collectionMetadata,
    editionAccount: collectionMasterEditionAccount,

    // other accounts
    bubblegumSigner: bubblegumSigner,
    compressionProgram: SPL_ACCOUNT_COMPRESSION_PROGRAM_ID,
    logWrapper: SPL_NOOP_PROGRAM_ID,
    tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
  },
  {
    metadataArgs: Object.assign(compressedNFTMetadata, {
      collection: { key: collectionMint, verified: false },
    }),
  },
);
```

Some of the other tree specific addresses are:

- `merkleTree` - the address of our tree we created
- `treeAuthority` - the authority of the tree
- `treeDelegate` - the delegated authority of the entire tree

Then we also have all of our NFT collection's addresses, including the mint
address, metadata account, and edition account. These addresses are also
standard to pass in when minting uncompressed NFTs.

#### Sign and send the transaction

Once our compressed mint instruction has been created, we can add it to a
transaction and send it to the Solana network:

```ts
const tx = new Transaction().add(compressedMintIx);
tx.feePayer = payer.publicKey;

// send the transaction to the cluster
const txSignature = await sendAndConfirmTransaction(connection, tx, [payer], {
  commitment: "confirmed",
  skipPreflight: true,
});
```

## Reading compressed NFTs metadata

With the help of a supporting RPC provider, developers can use the Digital Asset
Standard Read API (or "Read API" for short) to fetch the metadata of NFTs.

:::info

The Read API supports both compressed NFTs and traditional/uncompressed NFTs.
You can use the same RPC endpoints to retrieve all the assorted information for
both types of NFTs, including auto-fetching the NFTs' JSON URI.

:::

### Using the Read API

When working with the Read API and a supporting RPC provider, developers can
make `POST` requests to the RPC endpoint using your preferred method of making
such requests (e.g. `curl`, JavaScript `fetch()`, etc).

:::warning Asset ID

Within the Read API, digital assets (i.e. NFTs) are indexed by their `id`. This
asset `id` value differs slightly between traditional NFTs and compressed NFTs:

- for traditional/uncompressed NFTs: this is the token's address for the actual
  Account on-chain that stores the metadata for the asset.
- for compressed NFTs: this is the `id` of the compressed NFT within the tree
  and is **NOT** an actual on-chain Account address. While a compressed NFT's
  `assetId` resembles a traditional Solana Account address, it is not.

:::

### Common Read API Methods

While the Read API supports more than these listed below, the most commonly used
methods are:

- `getAsset` - get a specific NFT asset by its `id`
- `getAssetProof` - returns the merkle proof that is required to transfer a
  compressed NFT, by its asset `id`
- `getAssetsByOwner` - get the assets owned by a specific address
- `getAssetsByGroup` - get the assets by a specific grouping (i.e. a collection)

:::info Read API Methods, Schema, and Specification

Explore all the additional RPC methods added by Digital Asset Standard Read API
on [Metaplex's RPC Playground](https://metaplex-read-api.surge.sh/). Here you
will also find the expected inputs and response schema for each supported RPC
method.

:::

### Example Read API Request

For demonstration, below is an example request for the `getAsset` method using
the
[JavaScript Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API),
which is built into modern JavaScript runtimes:

```ts
// make a POST request to the RPC using the JavaScript `fetch` api
const response = await fetch(rpcEndpointUrl, {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
  },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: "rpd-op-123",
    method: "getAsset",
    params: {
      id: "5q7qQ4FWYyj4vnFrivRBe6beo6p88X8HTkkyVPjPkQmF",
    },
  }),
});
```

### Example Read API Response

With a successful response from the RPC, you should seem similar data to this:

```ts
{
  interface: 'V1_NFT',
  id: '5q7qQ4FWYyj4vnFrivRBe6beo6p88X8HTkkyVPjPkQmF',
  content: [Object],
  authorities: [Array],
  compression: [Object],
  grouping: [],
  royalty: [Object],
  creators: [],
  ownership: [Object],
  supply: [Object],
  mutable: false
}
```

The response fields to pay special attention to are:

- `id` - this is your asset's `id`
- `grouping` - can tell you the collection address that the NFT belongs to. The
  collection address will be the `group_value`.
- `metadata` - contains the actual metadata for the NFT, including the auto
  fetched JSON uri set when the NFT was minted
- `ownership` - gives you the NFT owner's address (and also if the NFT has
  delegated authority to another address)
- `compression` - tells you if this NFT is actually using compression or not.
  For compressed NFTs, this will also give you the tree address that is storing
  the compressed NFT on chain.

:::caution

Some of the returned values may be empty if the NFT is **not** a compressed NFT,
such as many of the `compression` fields. This is expected.

:::

## Transfer compressed NFTs

Transferring compressed NFTs is different from transferring uncompressed NFTs.
Aside from using a different on-chain program, compressed NFTs require the use
of a asset's "merkle proof" (or `proof` for short) to actually change ownership.

:::info What is a merkle proof?

An asset's "merkle proof" is a listing of all the "adjacent hashes" within the
tree that are required to validate a specific leaf within said tree.

These proof hashes themselves, and the specific asset's leaf data, are hashed
together in a deterministic way to compute the "root hash". Therefore, allowing
for cryptographic validation of an asset within the merkle tree.

**NOTE:** While each of these hash values resemble a Solana Account's
[address/public key](../../terminology.md#public-key-pubkey), they are not
addresses.

:::

Transferring ownership of a compressed NFT happens in 5 broad steps:

1. get the NFT's "asset" data (from the indexer)
2. get the NFT's proof (from the indexer)
3. get the Merkle tree account (directly from the Solana blockchain)
4. prepare the asset proof
5. build and send the transfer instruction

The first three steps primarily involve gathering specific pieces of information
(the `proof` and the tree's canopy depth) for the NFT to be transferred. These
pieces of information are needed to correctly parse/format the `proof` to
actually be sent within the transfer instruction itself.

### Get the asset

To perform the transfer of our compressed NFT, we will need to retrieve a few
pieces of information about the NFT.

For starters, we will need to get some the asset's information in order to allow
the on-chain compression program to correctly perform validation and security
checks.

We can use the `getAsset` RPC method to retrieve two important pieces of
information for the compressed NFT: the `data_hash` and `creator_hash`.

#### Example response from the `getAsset` method

Below is an example response from the `getAsset` method:

```ts
compression: {
  eligible: false,
  compressed: true,
  data_hash: 'D57LAefACeaJesajt6VPAxY4QFXhHjPyZbjq9efrt3jP',
  creator_hash: '6Q7xtKPmmLihpHGVBA6u1ENE351YKoyqd3ssHACfmXbn',
  asset_hash: 'F3oDH1mJ47Z7tNBHvrpN5UFf4VAeQSwTtxZeJmn7q3Fh',
  tree: 'BBUkS4LZQ7mU8iZXYLVGNUjSxCYnB3x44UuPVHVXS9Fo',
  seq: 3,
  leaf_id: 0
}
```

### Get the asset proof

The next step in preparing your compressed NFT transfer instruction, is to get a
**valid** asset `proof` to perform the transfer. This proof is required by the
on-chain compression program to validate on-chain information.

We can use the `getAssetProof` RPC method to retrieve two important pieces of
information:

- `proof` - the "full proof" that is required to perform the transfer (more on
  this below)
- `tree_id` - the on-chain address of the compressed NFTs tree

:::info Full proof is returned

The `getAssetProof` RPC method returns the complete listing of "proof hashes"
that are used to perform the compressed NFT transfer. Since this "full proof" is
returned from the RPC, we will need to remove the portion of the "full proof"
that is stored on-chain via the tree's `canopy`.

:::

#### Example response from the `getAssetProof` method

Below is an example response from the `getAssetProof` method:

```ts
{
  root: '7dy5bzgaRcUnNH2KMExwNXXNaCJnf7wQqxc2VrGXy9qr',
  proof: [
    'HdvzZ4hrPEdEarJfEzAavNJEZcCS1YU1fg2uBvQGwAAb',
    ...
    '3e2oBSLfSDVdUdS7jRGFKa8nreJUA9sFPEELrHaQyd4J'
  ],
  node_index: 131072,
  leaf: 'F3oDH1mJ47Z7tNBHvrpN5UFf4VAeQSwTtxZeJmn7q3Fh',
  tree_id: 'BBUkS4LZQ7mU8iZXYLVGNUjSxCYnB3x44UuPVHVXS9Fo'
}
```

### Get the Merkle tree account

Since the `getAssetProof` will always return the "full proof", we will have to
reduce it down in order to remove the proof hashes that are stored on-chain in
the tree's canopy. But in order to remove the correct number of proof addresses,
we need to know the tree's `canopyDepth`.

Once we have our compressed NFT's tree address (the `tree_id` value from
`getAssetProof`), we can use the
[`ConcurrentMerkleTreeAccount`](https://solana-labs.github.io/solana-program-library/account-compression/sdk/docs/classes/index.ConcurrentMerkleTreeAccount.html)
class, from the `@solana/spl-account-compression` SDK:

```ts
// retrieve the merkle tree's account from the blockchain
const treeAccount = await ConcurrentMerkleTreeAccount.fromAccountAddress(
  connection,
  treeAddress,
);

// extract the needed values for our transfer instruction
const treeAuthority = treeAccount.getAuthority();
const canopyDepth = treeAccount.getCanopyDepth();
```

For the transfer instruction, we will also need the current `treeAuthority`
address which we can also get via the `treeAccount`.

### Prepare the asset proof

With our "full proof" and `canopyDepth` values on hand, we can correctly format
the `proof` to be submitted within the transfer instruction itself.

Since we will use the `createTransferInstruction` helper function from the
Bubblegum SDK to actually build our transfer instruction, we need to:

- remove the proof values that are already stored on-chain in the
  [tree's canopy](../../learn/state-compression.md#canopy-depth), and
- convert the remaining proof values into the valid `AccountMeta` structure that
  the instruction builder function accepts

```ts
// parse the list of proof addresses into a valid AccountMeta[]
const proof: AccountMeta[] = assetProof.proof
  .slice(0, assetProof.proof.length - (!!canopyDepth ? canopyDepth : 0))
  .map((node: string) => ({
    pubkey: new PublicKey(node),
    isSigner: false,
    isWritable: false,
  }));
```

In the TypeScript code example above, we are first taking a `slice` of our "full
proof", starting at the beginning of the array, and ensuring we only have
`proof.length - canopyDepth` number of proof values. This will remove the
portion of the proof that is already stored on-chain in the tree's canopy.

Then we are structuring each of the remaining proof values as a valid
`AccountMeta`, since the proof is submitted on-chain in the form of "extra
accounts" within the transfer instruction.

### Build the transfer instruction

Finally, with all the required pieces of data about our tree and compressed
NFTs, and a correctly formatted proof, we are ready to actually create the
transfer instruction.

Build your transfer instruction using the
[`createTransferInstruction`](https://metaplex-foundation.github.io/metaplex-program-library/docs/bubblegum/functions/createTransferInstruction.html)
helper function from the Bubblegum SDK:

```ts
// create the NFT transfer instruction (via the Bubblegum package)
const transferIx = createTransferInstruction(
  {
    merkleTree: treeAddress,
    treeAuthority,
    leafOwner,
    leafDelegate,
    newLeafOwner,
    logWrapper: SPL_NOOP_PROGRAM_ID,
    compressionProgram: SPL_ACCOUNT_COMPRESSION_PROGRAM_ID,
    anchorRemainingAccounts: proof,
  },
  {
    root: [...new PublicKey(assetProof.root.trim()).toBytes()],
    dataHash: [...new PublicKey(asset.compression.data_hash.trim()).toBytes()],
    creatorHash: [
      ...new PublicKey(asset.compression.creator_hash.trim()).toBytes(),
    ],
    nonce: asset.compression.leaf_id,
    index: asset.compression.leaf_id,
  },
  BUBBLEGUM_PROGRAM_ID,
);
```

Aside from passing in our assorted Account addresses and the asset's proof, we
are converting the string values of our `data_hash`, `creator_hash`, `root` hash
into an array of bytes that is accepted by the `createTransferInstruction`
helper function.

Since each of these hash values resemble and are formatted similar to
PublicKeys, we can use the
[`PublicKey`](https://solana-labs.github.io/solana-web3.js/classes/PublicKey.html)
class in web3.js to convert them into a accepted byte array format.

#### Send the transaction

With our transfer instructions built, we can add it into a transaction and send
it to the blockchain similar to before. Making sure either the current
`leafOwner` or the `leafDelegate` signs the transaction.

:::note

After each successful transfer of a compressed NFT, the `leafDelegate` should
reset to an empty value. Meaning the specific asset will not have delegated
authority to an address other than its owner.

:::

And once confirmed by the cluster, we will have successfully transferred a
compressed NFT.

## Example code repository

You can find an example code repository for this developer guide on the Solana
Developers GitHub: https://github.com/solana-developers/compressed-nfts
