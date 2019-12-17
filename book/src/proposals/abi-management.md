# Solana ABI management process

This document proposes Solana ABI management process. The ABI management process
is an engineering practice and supporting technical framework to avoid
introducing unintended incompatible ABI changes.

# Problem

The Solana ABI (binary interface to the cluster) is currently only defined 
implicitly by the implementation, and requires a very careful eye to notice
breaking changes. This makes it extremely difficult to upgrade the software
on an existing cluster without rebooting the ledger.

# Requirements and objectives

- Unintended ABI changes can be detected as CI failures mechanically.
- Newer impl must be able to process oldest data (since genesis) once we go
  mainnet. (This is in stark contrast with regard to conventional software;
  A decade-old SSL 3.0 doesn't work anymore, but the genesis block of Bitcoin
  is still replayable today):
  - Deserialization stability: Newer implementation (like system programs)
    processes past object(transactions) exactly in the identical way when
    replaying the ledger.
  - Serialization stability: Newer implementation (like AccountsDB) produces
    newer blobs exactly in the identical way unless its ABI is explicitly and
    knowingly changed.
- The objective of this proposal is to protect the ABI while sustaining rather
  rapid development by opting for mechanical process rather than very long
  human-driven auditing process. After all, while cluster instability must be
  avoided, timely delivery of software improvements is desired for all
  stakeholders of the Solana cluster.
- Once cryptographically-signed, signed data blob must be identical, so no
  in-place data format update is possible including inbound and outbound of the
  online system. Also, considering the sheer volume of transactions we're aiming
  to handle, retrospective in-place update is undesirable at best.

# Solution

Instead of natural human's eye due-diligence, which should be assumed to fail
regularly, we need a systematic assurance of not breaking the cluster when
changing the source code.

For that purpose, we propose a two-fold mechanism of marking every ABI-related
things in source code (`struct`, `enums`) with the new `#[solana_abi]` attribute
and associating ABI-focussed unit tests with them.

The attribute try to detect any unsanctioned changes to the marked ABI-related
things when tests are run locally or on the CI, gathering closer attentions
from engineers further on. However, the detection cannot be complete; no matter
how hard we statically analyze the source code, it's still possible to break
ABI.

Thus, we confirm non-breakage by running paired tests for each ABI-related
things which are separated for both serialization and deserialization and are
versioned by the use of test fixtures containing actual binaries.

The focus here is on the compatibility of cluster functioning, but the
compatibility for non-critical parts should also be covered with this 
management process.

Additionally, binary test fixtures could function as the single source of truth
of executable specification of Solana's binary format and compatibility.

# Quick observational recap of problem for the solution

ABI compatibility can be boiled down to the problem of how we serialize a part
of runtime data. Then, the serialized data must be consumable by newer or older
runtime depending on the circumstances.

This is a general problem definition, applicable regardless of the area of
system components, including serializing into files, sockets and smart
contracts.

Thus, as a rather straight logical conclusion, we design an unit testing 
mechanism targeting at the exact boundaries of the version difference and
focusing on only serializing and deserializing.

Also this problem generality could expand, to some extent, into the
bidirectional communications across cluster nodes in the network and between
runtime and contract as well. That's because these communication flows should
be deterministic due to the Solana's nature of being a DLT meaning these are
reproducibly recordable and replayable to test each side separately.

# Definitions

Application binary interface (ABI): every system boundary which needs
serialization/deserialization to communicate across the boundary of system
components. This means sending and receiving message with peer nodes via
networks and storing and loading files to storage devices.

ABI item/type: any conceivable thing and its associated kind, which collectively
comprises the whole ABI for any system components. For example, type includes
structs, serializer logics and implemented protocol communication procedures.

ABI item digest: Some hash based on its fields' type information. Used casual
detection of unintended possible ABI change.

ABI item revision: Monotonic increasing integer starting with 0, precisely
identifying specific implementation at any range of time in the past or in the
(near) future. All existing ABI item is unversioned and regarded as being
revision 0.

Versioning: Start adding variant of behaviors for given ABI item and starting to
increment revision by 1.

Revision list: List of revisions for given ABI item. Each list element has an
associated range of time in some kind of units (epoch, cluster version, data
format magic number) and other meta data for each revision like compatibility
statuses. The time range could be overlapping or non-overlapping.

Compatibility: Being able to process information from external source according
to the specified behavior with the deterministic outcome.

Compatibility status: What kind of strictness of system component should be
desired when new (=not explicitly recognized) form of information is
encountered. This can be classified as ignore/reject.

Maintenance tiers: How incompatible changes should be introduced to the
cluster. This means either (planned) hard-fork, urgent security fix requiring
ABI change (Tier-1), or normal periodical voluntary software update (Tier-2).
Tier-1 includes transactions and accounts, for example. Tier-2 includes shreds
and snapshots, for example.

Hard fork: A hard fork is a change to the cluster that makes previously invalid
communications valid, and therefore requires all users / software to upgrade
(definition is based on [1]). The upgrade is programmed and scheduled and takes
place effective at specific future epoch.

# Examples

## Example 1: If we were adding #7233 after some time since mainnet launch:

```patch
@@applicaton code@@

 #[solana_abi(allow=add, digest="79caeee9")]
   // ^^ adding a member to allow=add enum doesn't trigger digest change
 #[derive(SolanaABI)]
 pub enum VoteError {
     ....
+
+    #[error("vote timestamp not recent")]
+    TimestampTooOld,
 }

-#[solana_abi(allow=none, digest="3ffc9921")]
+#[solana_abi(allow=none, digest="1c6a53e9")]
 #[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)] 
 pub struct Vote {
     /// A stack of votes starting with the oldest vote
     pub slots: Vec<Slot>,
     /// signature of the bank's state at the last slot
     pub hash: Hash,
+    #[solana_abi_added(since = "rev1")]
+    /// processing timestamp of last slot
+    pub timestamp: Option<UnixTimestamp>,
 }

 impl SolanaABI for Vote {
   const REVISION_LIST = [
      (0, "epoch", 0..), // leave this revision acceptable for indefinite time
+     (1, "epoch", 21..),
   ]
 }


@@test code@@
 #[test]
 #[solana_abi_test]
 test_serialize_vote() {
    //   ^^^ no change is needed for ABI test case
    let vote = ...
    serializing_with_fixture!(vote, |write_stream, revision_tuple| {
        // use revision_tuple to differentiate serialization in some way...
        serialize_into(write_stream, normal_vote)
    });
    ...
 }

 #[test]
 #[solana_abi_test]
 test_deserialize_vote() {
    //   ^^^ no change is needed for ABI test case
    let vote = ...
    deserializing_with_fixture!(Ok(vote), |read_stream, revision_tuple| {
        // use revision_tuple to differentiate deserialization in some way..
        deserialize_from(write_stream)
    });
 }

Files:
 test/fixtures/vote-rev-0.hex.yaml
+test/fixtures/vote-rev-1.hex.yaml
```

## Example 2: If we were adding #7415 after some time since mainnet launch:

```patch
 /// Meta contains enough context to recover the index from storage itself
 /// This struct will be backed by mmaped and snapshotted data files.
 /// So the data layout must be stable and consistent across the entire cluster!
-#[solana_abi(allow=none, digest="662792c9", test="test_hash_stored_account")]
+#[solana_abi(allow=none, digest="c5031113", test="test_hash_stored_account")]
 #[derive(PartialEq, Debug)]
 pub struct StoredAccount<'a> {
     pub meta: &'a StoredMeta,
     /// account data
     pub account_meta: &'a AccountMeta,
     pub data: &'a [u8],
     pub offset: usize,
+    #[solana_abi_changed(since = "rev1", comment="mix in rent_epoch and executable")
     pub hash: &'a Hash,
 }

 impl SolanaABI for StoredAccount {
   const REVISION_LIST = [
-     (0, "cluster", "v0.1.1"..),
+     (1, "cluster", "v0.1.1".."v1.3.0"), // Reject/ignore after some time
+     (2, "cluster", "v1.2.0"..),
   ]
   # or
   const REVISION_LIST = [
-     (0, "snapshot_format", "1.."),
+     (1, "snapshot_format", "1"),
+     (2, "snapshot_format", "2.."),
   ]

  fn hash_account_data(
      ...,
      snapshot_format_version: u8,
  ) -> Hash {
        LittleEndian::write_u64(&mut buf[..], slot);
        hasher.hash(&buf);

        if (snapshot_format_version >= 2) { 
            LittleEndian::write_u64(&mut buf[..], rent_epoch);
            hasher.hash(&buf);
        }

        hasher.hash(&data);
        ...
   }

   ...

   // no change for ABI test is needed.
Files:
 .../fixtures/stored-account-rev-0.hex.yaml
+.../fixtures/stored-account-rev-1.hex.yaml
```

# Compatibility strictness

(To be documented: table of compatibility of compatibility item types, status
and kind of changes)

# Hard fork work flow

(To be documented.)

# Complehensive test suite of golden binary ledgers

(To be documented: Maintain ledger generation script which executes tx once for
each program?  While we should verify we can consume older ledgers, we also
should verify we can produce logically identical ledgers from scratch.)

# Relationale

- why need to write unit test?: For example crates can introduce unintended
  changes accidentally. e2e-like testing for serialization is needed. Also
  these large collection of small unit tests could be used for fuzzing testing
  like afl-fuzz as the entry points for various untrusted input attack surfaces.

- why `revision'?: Introduced new indirection to capture different
  timelines for diffent ABI items and units (epoch, version, etc). The
  `revision` is chosen because of `git grep revision` -> `Exited with 1`

- why fixture?: to run the same test code repeatedly with several input
  variations (all possible revisions). 

- why for each revision? what about fixture maintenance burden?: ABI changes
  should be last resorts in general (even adding fields, in contrast to the RPC
  API), so the pace of increasing number of fixtures should be low.

- why ascii binary (= hex) embedded inside yaml as the test fixture?
  yaml is line oriented and expressive adequately for this case. We can use
  its flexible formatting to document format spec informally but sufficiently.
  (See example for details) Also, we need to maintain expected success/failure
  status for each test fixture (=revision), so direct hex/bin file isn't enough.
  Also, yaml is used already for our codebase.

# Other specifications

## Directory structure

.../fixtures/vote-initialize-tx-rev1.hex.yaml
.../fixtures/vote-initialize-tx-rev2.hex.yaml
.../fixtures/vote-vote-update-node-id-tx.hex.yaml

## .hex.yaml file (test fixture) syntax

A fixture for specific revision, which got unsupported:

```yaml
# the hex values are all concatnated together; can be arbitary folded for 
# document?
data:
  cafe # comment
  cafe
  beafbeaf ........
status:
  epoch:
    0..: Ok
    92..: Rejected
```

A fixture for specific revision, which got supported:

```yaml
# the hex values are all concatnated together; can be arbitary folded for 
# document?
data:
  cafe # comment
  cafe
  beafbeaf ........
status:
  cluster_version:
    "v0.1.1..": Ignored # or Rejected
    "v0.32.0..": Ok
```

## API 

```Rust
pub fn serializing_with_fixture(|write_stream, revision| {
  // serialize here according to revision
  Ok(...)/Err(...)
});// internally asserts against binary in the fixture for equality or assert status.

pub fn deserializing_with_fixture(normal_result, |read_stream, revision| {
  // deserialize and post-process according to revision
  Ok(...)/Err(...)
});// internally asserts against given result for equality or assert status.
```

## Assertion steps 

### for backward compatibility

We can make sure we can serialize/deserialize for given change in this way:

1. Run deserialize test with fixture of REV-1, REV-2, ..., REV0
2. Run serialize test with fixture of REV-1, REV-2, ..., REV0

### for forward compatibility

We can make sure mixed versions of cluster doesn't break for given change in
this way:

1. Only backport the newer test fixtures without application code changes
2. Run deserialize test with fixture of REV+1, REV+2, ... REV+LATEST
3. Run serialize test with fixture of REV+1, REV+2, ... REV+LATEST


# Developer's work flow

We will end up accumulating increasing number of technical debt over time to
support older ABI item revisions. As a general rule of thumb for implementation
design, we always normalize the older serialized data into the runtime/internal
representation which closely reflects the latest revision of given ABI item as
much as possible with additional conversion logic.

Because it's more expensive to maintain this ABI test than normal one, normal
unit test is preferred for general unit testing as before. ABI unit test should
be restricted to the bare minimal set of variation of serialization.

## Change an existing compatibility item:

1. Change `#[solana_abi]` `struct`s as usually
2. Depending on its maintenance tier, decide hard-fork or normal update
3. (For hard-fork,) decide scheduled release time based on epochs
4. Write the code to differentiate depending on
   [version](#how to detect versions)
5. Generate new test fixture for the new compatibility item revision and check
   the diff.

## Create new compatibility item:

1. As there is no free standing system component, this thing is actually
   related to the other part of system in some way. Consider what compatibility
   status and maintenance tier is expected for the relation regarding adding
   something new.
2. Write entirely new struct/module code with `#[solana_abi]`
3. Write entirely new serialization/deserialization unit test code
4. Generate fixture for the first revision and check the diff.

## How to generate test fixtures for newer versions

run `cargo test` with specific environment like `SOLANA_UPDATE_ABI_FIXTURES=1`.
Then, these fixtures will be automatically updated and review with `git diff`

# Implemention remarks

We'll end up heavily relying on macro magics. For precedent, `ink' from the
Parity Technologies [2], could be informational.

# Considerations
 
- how to detect vesions?
  - Introduce version header? (2 byte for everything)
  - Implicit version header by optimistic signature verification with version as
    salt? This detects the version by first verifying signature for blob with
    (salt=latest revision) and if failed try with (salt=latest revision-1) and
    so on.
- how to calculate type digest?
  - sha1 tuple of (field names, type names), don't consider non derived
    (De)Serializer; tolerate for incompleteness as mentioned
- how to integrate well with existing serialization libraries (ie. serde,
  bincode)?
- We currently only support amd64, there might be unforseen ABI problems for
  supporting other architectures like little/big endian, floting point numbers.
- How about generic types?: currently we only consider concrete types

# Prior works

As far as I researched, I couldn't find good proven standard approach to the ABI
management, to my surprise. It seems that ad-hoc or integration-test level ABI
management is common in the scene. However, I think these systematic management
will provide confidene of changes and allow rapid development.

# References

1. Bitcoin Wiki: https://en.bitcoin.it/wiki/Hardfork
2. Prity's ink to write smart contracts: https://github.com/paritytech/ink
