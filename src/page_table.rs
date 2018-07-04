/// The `page_table` module implements a fast way to process requests to smart contracts including
/// simple transactions.  Benchmarks show about 125,791 ns/iter of 256 Tx's which is near 2m TPS in
/// single threaded mode.
///
/// This can be safely pipelined.  The memory lock ensures that all pages
/// traveling through the system are non overlapping, and using the WRITE lock durring allocation
/// ensures that they are present when the READ lock is held.  To safely execute the contracts in
/// parallel the READ lock must be held while loading the pages, and executing the contract.
///
///
/// Main differences from the `Bank`
/// 1. `last_count` and `last_hash` are PoH identifiers, these are processed outside of this pipeline
/// 2. `Call.version` is used to prevent duplicate spends, each PublicKey has 2**64 number of calls
/// it can make.
/// 3. `Page` entry is similar to an `Account`, but it also has a `contract` that owns it.  That
///    tag allows the contract to Write to the memory owned by the page.  Contracts can spend money
use bank::{Bank, BANK_PROCESS_TRANSACTION_METHOD};
use bincode::{deserialize, serialize};
use hash::{hash,Hash};
use rand::{thread_rng, Rng, RngCore};
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature};
use std::collections::{BTreeMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};

//run with 'cargo +nightly bench --features=unstable,parex page_table::bench'
#[cfg(feature = "parex")]
use rayon::prelude::*;

//these types vs just u64 had a 40% impact on perf without FastHasher
//type Hash = [u64; 4];
//type PublicKey = [u64; 4];
//type Signature = [u64; 8];
type ContractId = [u64; 4];

pub const DEFAULT_CONTRACT: ContractId = [0u64; 4];

/// SYSTEM interface, same for very contract, methods 0 to 127
/// method 0
/// reallocate
/// spend the funds from the call to the first recepient
pub fn system_0_realloc(call: &Call, pages: &mut [Page]) {
    if call.data.contract == DEFAULT_CONTRACT {
        let size: u64 = deserialize(&call.data.user_data).unwrap();
        pages[0].memory.resize(size as usize, 0u8);
    }
}
/// method 1
/// assign
/// assign the page to a contract
pub fn system_1_assign(call: &Call, pages: &mut [Page]) {
    let contract: ContractId = deserialize(&call.data.user_data).unwrap();
    if call.data.contract == DEFAULT_CONTRACT {
        pages[0].contract = contract;
        //zero out the memory in pages[0].memory
        //Contracts need to own the state of that data otherwise a use could fabricate the state and
        //manipulate the contract
        pages[0].memory.clear();
    }
}
/// DEFAULT_CONTRACT interface
/// All contracts start with 128
/// method 128
/// move_funds
/// spend the funds from the call to the first recepient
pub const DEFAULT_CONTRACT_MOVE_FUNDS: u8 = 128;
pub fn default_contract_128_move_funds(call: &Call, pages: &mut [Page]) {
    let amount: i64 = deserialize(&call.data.user_data).unwrap();
    pages[0].balance -= amount;
    pages[1].balance += amount;
}

#[cfg(test)]
pub fn default_contract_254_create_new_other(call: &Call, pages: &mut [Page]) {
    let amount: i64 = deserialize(&call.data.user_data).unwrap();
    pages[1].balance += amount;
}

#[cfg(test)]
pub fn default_contract_253_create_new_mine(call: &Call, pages: &mut [Page]) {
    let amount: i64 = deserialize(&call.data.user_data).unwrap();
    pages[0].balance += amount;
}

//157,173 ns/iter vs 125,791 ns/iter with using this hasher, 110,000 ns with u64 as the key
struct FastHasher {
    //generates some seeds to pluck bytes out of the keys
    //since these are random every time we create a new hashset, there is no way to
    //predict what these will be for the node or the network
    rand32: [usize; 8],
    rand8: [usize; 8],
    data64: [u8; 64],
    len: usize,
}

impl Hasher for FastHasher {
    /// pluck the bytes out of the data and glue it into a u64
    fn finish(&self) -> u64 {
        let seed = if self.len == 32 {
            self.rand32
        } else if self.len == 8 {
            self.rand8
        } else {
            assert!(false);
            [0xfefe_fefe_fefe_fefe; 8]
        };
        assert_eq!(seed.len(), 8);
        (self.data64[seed[0]] as u64)
            | (u64::from(self.data64[seed[1]]) << (8))
            | (u64::from(self.data64[seed[2]]) << (2 * 8))
            | (u64::from(self.data64[seed[3]]) << (3 * 8))
            | (u64::from(self.data64[seed[4]]) << (4 * 8))
            | (u64::from(self.data64[seed[5]]) << (5 * 8))
            | (u64::from(self.data64[seed[6]]) << (6 * 8))
            | (u64::from(self.data64[seed[7]]) << (7 * 8))
    }
    fn write(&mut self, bytes: &[u8]) {
        assert!(bytes.len() < 64);
        self.data64[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len();
    }
}

impl Default for FastHasher {
    fn default() -> FastHasher {
        fn gen_rand(size: usize, rand: &mut [usize]) {
            let mut seed: Vec<_> = (0..size).collect();
            assert!(seed.len() >= 8);
            thread_rng().shuffle(&mut seed);
            rand[0..8].copy_from_slice(&seed[0..8]);
        }

        let mut rand32 = [0usize; 8];
        gen_rand(32, &mut rand32);
        let mut rand8 = [0usize; 8];
        gen_rand(8, &mut rand8);

        FastHasher {
            rand32,
            rand8,
            data64: [0; 64],
            len: 0,
        }
    }
}

impl BuildHasher for FastHasher {
    type Hasher = FastHasher;
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher {
            rand32: self.rand32,
            rand8: self.rand8,
            data64: [0; 64],
            len: 0,
        }
    }
}

/// Generic Page for the PageTable
#[derive(Clone)]
pub struct Page {
    /// key that indexes this page
    pub owner: PublicKey,
    /// contract that owns this page
    /// Only the contract can write to the `memory` vector
    pub contract: ContractId,
    /// balance that belongs to owner
    pub balance: i64,
    /// Version used for reply attacks
    /// The `CallData::version` field must match the key[0]'s Page::version field
    /// Once its processed, the version number is incremented.
    pub version: u64,
    /// signature of the last transaction that was called with this page as the primary key
    pub signature: Signature,
    /// hash of the page data
    pub memhash: Hash,
    /// The following could be in a separate structure
    pub memory: Vec<u8>,
}

impl Page {
    pub fn new(owner: PublicKey, contract: ContractId, balance: i64) -> Page {
        Page {
            owner,
            contract,
            balance,
            version: 0,
            signature: Signature::default(),
            memhash: Hash::default(),
            memory: vec![],
        }
    }
}
impl Default for Page {
    fn default() -> Page {
        Page {
            owner: PublicKey::default(),
            contract: DEFAULT_CONTRACT,
            balance: 0,
            version: 0,
            signature: Signature::default(),
            memhash: Hash::default(),
            memory: vec![],
        }
    }
}
/// Call definition
/// Signed portion
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CallData {
    // TODO(anatoly): allow for read only pages
    /// number of keys to load, aka the to key
    /// keys[0] is the caller's key
    pub keys: Vec<PublicKey>,

    /// the set of PublicKeys that are required to have a proof
    pub required_proofs: Vec<u8>,

    /// PoH data
    /// last id PoH observed by the sender
    pub last_count: u64,
    /// last PoH hash observed by the sender
    pub last_hash: Hash,

    /// Program
    /// the address of the program we want to call
    pub contract: ContractId,
    /// OS scheduling fee
    pub fee: i64,
    /// struct version to prevent duplicate spends
    /// Calls with a version <= Page.version are rejected
    pub version: u64,
    /// method to call in the contract
    pub method: u8,
    /// usedata in bytes
    pub user_data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Call {
    /// Signatures and Keys
    /// (signature, key index)
    /// This vector contains a tuple of signatures, and the key index the signature is for
    /// proofs[0] is always key[0]
    pub proofs: Vec<Signature>,
    pub data: CallData,
}

/// simple transaction over a Call
/// TODO: The pipeline will need to pass the `destination` public keys in a side buffer to generalize
/// this
impl Call {
    /// Create a new Call object
    pub fn new(
        keys: Vec<PublicKey>,
        required_proofs: Vec<u8>,
        last_count: u64,
        last_hash: Hash,
        contract: ContractId,
        version: u64,
        fee: i64,
        method: u8,
        user_data: Vec<u8>,
    ) -> Self {
        Call {
            proofs: vec![],
            data: CallData {
                keys,
                required_proofs,
                last_count,
                last_hash,
                contract,
                fee,
                version,
                method,
                user_data,
            },
        }
    }
    pub fn new_tx(
        from: PublicKey,
        last_count: u64,
        last_hash: Hash,
        amount: i64,
        fee: i64,
        version: u64,
        to: PublicKey,
    ) -> Self {
        let user_data = serialize(&amount).unwrap();
        Self::new(
            vec![from, to],
            vec![0],
            last_count,
            last_hash,
            DEFAULT_CONTRACT,
            version,
            fee,
            DEFAULT_CONTRACT_MOVE_FUNDS,
            user_data,
        )
    }

    /// Get the transaction data to sign.
    pub fn get_sign_data(&self) -> Vec<u8> {
        serialize(&self.data).expect("serialize CallData")
    }

    pub fn append_signature(&mut self, keypair: &KeyPair) {
        let sign_data = self.get_sign_data();
        let signature = Signature::new(keypair.sign(&sign_data).as_ref());
        if self.proofs.len() >= self.data.keys.len()
            || Some(&keypair.pubkey()) != self.data.keys.get(self.proofs.len())
        {
            warn!("signature is for an invalid pubkey");
        }
        self.proofs.push(signature);
    }
    /// Verify only the transaction signature.
    pub fn verify_sig(&self) -> bool {
        warn!("slow transaction signature verification called");
        let sign_data = self.get_sign_data();
        self.proofs
            .iter()
            .zip(&self.data.required_proofs)
            .map(|(sig, index)| sig.verify(&self.data.keys[*index as usize].as_ref(), &sign_data))
            .all(|y| y)
    }

    fn rand_key() -> PublicKey {
        let mut r = thread_rng();
        let mut data = [0u8; 32];
        r.fill_bytes(&mut data);
        PublicKey::new(&data)
    }
    fn rand_sig() -> Signature {
        let mut r = thread_rng();
        let mut data = [0u8; 64];
        r.fill_bytes(&mut data);
        Signature::new(&data)
    }
    fn rand_hash() -> Hash {
        let mut r = thread_rng();
        let mut data = [0u8; 32];
        r.fill_bytes(&mut data);
        hash(&data)
    } 
    pub fn random_tx() -> Call {
        //sanity check
        assert_ne!(Self::rand_key(), Self::rand_key());
        let mut tx = Self::new_tx(
            Self::rand_key(),
            0,
            Self::rand_hash(),
            1,
            1,
            0,
            Self::rand_key(),
        );
        tx.proofs.push(Self::rand_sig());
        tx
    }
}

pub struct AllocatedPages {
    max: usize,
    allocated: BTreeMap<PublicKey, usize>,
    free: Vec<usize>,
}
impl Default for AllocatedPages {
    fn default() -> Self {
        AllocatedPages {
            max: 0,
            allocated: BTreeMap::new(),
            free: vec![],
        }
    }
}
impl AllocatedPages {
    pub fn lookup(&self, key: &PublicKey) -> Option<usize> {
        self.allocated.get(key).cloned().map(|x| x as usize)
    }

    pub fn free(&mut self, key: &PublicKey) {
        let page = self.lookup(key).unwrap();
        self.free.push(page);
        self.allocated.remove(key);
    }
    pub fn allocate(&mut self, key: PublicKey) -> usize {
        let page = if let Some(p) = self.free.pop() {
            p
        } else {
            let p = self.max;
            self.max += 1;
            p
        };
        let old = self.allocated.insert(key, page);
        assert!(old.is_none());
        page
    }
}

pub struct PageTable {
    /// a map from page public keys, to index into the page_table
    allocated_pages: RwLock<AllocatedPages>,
    // TODO(anatoly): allow for read only pages
    // read only pages would need a ref count per page, so a HashMap is probably a better structure
    /// locked pages that are currently processed
    mem_locks: Mutex<HashSet<PublicKey, FastHasher>>,
    /// entries of Pages
    page_table: RwLock<Vec<Page>>,
    transaction_count: AtomicUsize,
}

pub const N: usize = 256;
pub const K: usize = 16;
pub struct Context {
    pub valid_ledger: Vec<bool>,
    pub lock: Vec<bool>,
    needs_alloc: Vec<bool>,
    pub checked: Vec<bool>,
    pages: Vec<Vec<Option<usize>>>,
    loaded_page_table: Vec<Vec<Page>>,
    pub commit: Vec<bool>,
}
impl Default for Context {
    fn default() -> Self {
        let valid_ledger = vec![false; N];
        let lock = vec![false; N];
        let needs_alloc = vec![false; N];
        let checked = vec![false; N];
        let pages = vec![vec![None; K]; N];
        let loaded_page_table: Vec<Vec<_>> = vec![vec![Page::default(); K]; N];
        let commit = vec![false; N];
        Context {
            valid_ledger,
            lock,
            needs_alloc,
            checked,
            pages,
            loaded_page_table,
            commit,
        }
    }
}

impl Default for PageTable {
    fn default() -> Self {
        PageTable {
            allocated_pages: RwLock::new(AllocatedPages::default()),
            mem_locks: Mutex::new(HashSet::with_hasher(FastHasher::default())),
            page_table: RwLock::new(vec![]),
            transaction_count: AtomicUsize::new(0),
        }
    }
}

impl PageTable {
    // for each call in the packet acquire the memory lock for the pages it references if possible
    pub fn acquire_memory_lock(
        &self,
        packet: &[Call],
        valid_ledger: &[bool],
        acquired_memory: &mut [bool],
    ) {
        //holds mem_locks mutex
        let mut mem_locks = self.mem_locks.lock().unwrap();
        for (i, tx) in packet.iter().enumerate() {
            acquired_memory[i] = false;
            if !valid_ledger[i] {
                continue;
            }
            let collision: u64 = tx.data
                .keys
                .iter()
                .map({ |k| mem_locks.contains(k) as u64 })
                .sum();
            trace!("contains count: {}", collision);
            acquired_memory[i] = collision == 0;
            if collision == 0 {
                for k in &tx.data.keys {
                    mem_locks.insert(*k);
                }
            }
        }
    }
    pub fn acquire_validate_find(&self, transactions: &[Call], ctx: &mut Context) {
        self.acquire_memory_lock(&transactions, &ctx.valid_ledger, &mut ctx.lock);
        self.validate_call(&transactions, &ctx.lock, &mut ctx.checked);
        self.find_new_keys(
            &transactions,
            &ctx.checked,
            &mut ctx.needs_alloc,
            &mut ctx.pages,
        );
    }
    pub fn allocate_keys_with_ctx(&self, transactions: &Vec<Call>, ctx: &mut Context) {
        self.allocate_keys(
            &transactions,
            &ctx.checked,
            &ctx.needs_alloc,
            &mut ctx.pages,
        );
    }
    pub fn execute_with_ctx(transactions: &Vec<Call>, ctx: &mut Context) {
        PageTable::execute(
            &transactions,
            &ctx.checked,
            &mut ctx.loaded_page_table,
            &mut ctx.commit,
        );
    }

    /// validate that we can process the fee and its not a dup call
    pub fn validate_call(
        &self,
        packet: &[Call],
        acquired_memory: &Vec<bool>,
        checked: &mut Vec<bool>,
    ) {
        let allocated_pages = self.allocated_pages.read().unwrap();
        let page_table = self.page_table.read().unwrap();
        for (i, tx) in packet.iter().enumerate() {
            checked[i] = false;
            if !acquired_memory[i] {
                continue;
            }
            let caller = &tx.data.keys[0];
            if let Some(memix) = allocated_pages.lookup(caller) {
                if let Some(page) = page_table.get(memix) {
                    assert_eq!(page.owner, *caller);
                    // skip calls that are to old
                    // this check prevents retransmitted transactions from being processed twice
                    // page_table[keys[0]].version
                    if page.version != tx.data.version {
                        trace!(
                            "version is not correct {} {}",
                            page.version,
                            tx.data.version
                        );
                        continue;
                    }
                    // caller page must belong to the contract
                    // TODO(anatoly): we could relax this, and have the contract itself
                    // do this check for pages it cares about
                    if page.contract != tx.data.contract {
                        trace!(
                            "contract is not correct {:x} {:x}",
                            page.contract[0],
                            tx.data.contract[0]
                        );
                        continue;
                    }
                    // caller page must cover the tx.data.fee
                    if page.balance >= tx.data.fee {
                        checked[i] = true;
                    }
                }
            }
        }
    }
    pub fn find_new_keys(
        &self,
        packet: &[Call],
        checked: &Vec<bool>,
        needs_alloc: &mut Vec<bool>,
        pages: &mut Vec<Vec<Option<usize>>>,
    ) {
        //holds READ lock to the page table
        let allocated_pages = self.allocated_pages.read().unwrap();
        for (i, tx) in packet.iter().enumerate() {
            if !checked[i] {
                continue;
            }
            needs_alloc[i] = false;
            for (j, k) in tx.data.keys.iter().enumerate() {
                let lookup = allocated_pages.lookup(k);
                needs_alloc[i] = needs_alloc[i] || lookup.is_none();
                pages[i][j] = lookup;
            }
        }
    }
    pub fn force_deposit_to(
        page_table: &mut Vec<Page>,
        allocated_pages: &mut AllocatedPages,
        owner: PublicKey,
        contract: ContractId,
        amount: i64,
    ) {
        let page = Page::new(owner, contract, amount);
        let ix = allocated_pages.allocate(owner) as usize;
        if page_table.len() <= ix {
            page_table.resize(ix + 1, page);
        } else {
            page_table[ix] = page;
        }
    }

    pub fn force_deposit(&self, owner: PublicKey, contract: ContractId, amount: i64) {
        let mut allocated_pages = self.allocated_pages.write().unwrap();
        let mut page_table = self.page_table.write().unwrap();
        Self::force_deposit_to(
            &mut page_table,
            &mut allocated_pages,
            owner,
            contract,
            amount,
        );
    }

    pub fn transaction_count(&self) -> usize {
        self.transaction_count.load(Ordering::Relaxed)
    }

    /// used for testing
    pub fn bump_versions(&self, packet: &[Call]) {
        let allocated_pages = self.allocated_pages.write().unwrap();
        let mut page_table = self.page_table.write().unwrap();
        packet.iter().for_each(|tx| {
            allocated_pages.lookup(&tx.data.keys[0]).map(|ix| {
                page_table.get_mut(ix).map(|page| {
                    page.version += 1;
                });
            });
        });
    }

    /// used for testing
    pub fn force_allocate(&self, packet: &Vec<Call>, owner: bool, amount: i64) {
        let mut allocated_pages = self.allocated_pages.write().unwrap();
        let mut page_table = self.page_table.write().unwrap();
        for tx in packet.iter() {
            let key = if owner {
                tx.data.keys[0]
            } else {
                tx.data.keys[1]
            };
            Self::force_deposit_to(
                &mut page_table,
                &mut allocated_pages,
                key,
                tx.data.contract,
                amount,
            );
        }
    }
    /// used for testing
    pub fn sanity_check_pages(
        &self,
        txs: &Vec<Call>,
        checked: &Vec<bool>,
        pages: &Vec<Vec<Option<usize>>>,
    ) {
        let page_table = self.page_table.read().unwrap();
        //while we hold the write lock, this is where we can create another anonymouns page
        //with copy and write, and send this table to the vote signer
        for (i, tx) in txs.iter().enumerate() {
            if checked[i] {
                assert!(pages[i][0].is_some());
            }
            for (p, k) in pages[i].iter().zip(&tx.data.keys) {
                if p.is_some() {
                    assert_eq!(*k, page_table[p.unwrap()].owner);
                }
            }
        }
    }
    pub fn allocate_keys(
        &self,
        packet: &Vec<Call>,
        checked: &Vec<bool>,
        needs_alloc: &Vec<bool>,
        pages: &mut Vec<Vec<Option<usize>>>,
    ) {
        let mut allocated_pages = self.allocated_pages.write().unwrap();
        let mut page_table = self.page_table.write().unwrap();
        for (i, tx) in packet.iter().enumerate() {
            if !checked[i] {
                continue;
            }
            if !needs_alloc[i] {
                continue;
            }
            for (j, key) in tx.data.keys.iter().enumerate() {
                if pages[i][j].is_some() {
                    continue;
                }
                let page = Page {
                    owner: *key,
                    contract: tx.data.contract,
                    version: 0,
                    balance: 0,
                    signature: Signature::default(),
                    memhash: Hash::default(),
                    memory: vec![],
                };
                let ix = allocated_pages.allocate(*key) as usize;
                //recheck since we are getting a new lock
                if page_table.len() <= ix {
                    trace!("reallocating page table {}", ix);
                    //safe to do while the WRITE lock is held
                    page_table.resize(ix + 1, Page::default());
                }
                page_table[ix] = page;
                pages[i][j] = Some(ix);
            }
        }
    }

    pub fn load_pages(
        &self,
        packet: &Vec<Call>,
        checked: &Vec<bool>,
        pages: &Vec<Vec<Option<usize>>>,
        loaded_page_table: &mut Vec<Vec<Page>>,
    ) {
        let page_table = self.page_table.read().unwrap();
        for (i, tx) in packet.into_iter().enumerate() {
            if !checked[i] {
                continue;
            }
            for (j, (_, oix)) in tx.data.keys.iter().zip(pages[i].iter()).enumerate() {
                let ix = oix.expect("checked pages should be loadable");
                loaded_page_table[i][j] = page_table[ix].clone();
            }
        }
    }
    pub fn load_pages_with_ctx(&self, packet: &Vec<Call>, ctx: &mut Context) {
        self.load_pages(packet, &ctx.checked, &ctx.pages, &mut ctx.loaded_page_table);
    }

    /// calculate the balances in the loaded pages
    /// at the end of the contract the balance must be the same
    /// and contract can only spend tokens from pages assigned to it
    fn validate_balances(tx: &Call, pre_pages: &Vec<Page>, post_pages: &Vec<Page>) -> bool {
        // contract can spend any of the tokens it owns
        for ((pre, post), _tx) in pre_pages
            .iter()
            .zip(post_pages.iter())
            .zip(tx.data.keys.iter())
        {
            if pre.contract != tx.data.contract && pre.balance <= post.balance {
                return false;
            }
        }
        // contract can't spend any of the tokens it doesn't own
        let pre_sum: i64 = pre_pages
            .iter()
            .zip(tx.data.keys.iter())
            .map(|(pre, _)| pre.balance)
            .sum();
        let post_sum: i64 = post_pages
            .iter()
            .zip(tx.data.keys.iter())
            .map(|(pre, _)| pre.balance)
            .sum();
        pre_sum == post_sum
    }

    /// parallel execution of contracts should be possible here since all the alls have no
    /// dependencies
    pub fn execute(
        // Pass the _allocated_pages argument to make sure the lock is held for this call
        packet: &Vec<Call>,
        checked: &Vec<bool>,
        loaded_page_table: &mut Vec<Vec<Page>>,
        commit: &mut Vec<bool>,
    ) {
        #[cfg(not(feature = "parex"))]
        let iter = packet
            .into_iter()
            .zip(loaded_page_table)
            .zip(checked)
            .zip(commit);
        #[cfg(feature = "parex")]
        let iter = packet
            .into_par_iter()
            .zip(loaded_page_table)
            .zip(checked)
            .zip(commit);

        iter.for_each(|(((tx, loaded_pages), checked), commit)| {
            *commit = false;
            if !checked {
                return;
            }
            // fee is paid
            *commit = true;
            loaded_pages[0].balance -= tx.data.fee;
            //update the version
            //TODO(anatoly): TODO wrap around test
            loaded_pages[0].version += 1;
            loaded_pages[0].signature = tx.proofs[0];

            let mut call_pages: Vec<Page> = tx.data
                .keys
                .iter()
                .zip(loaded_pages.iter())
                .map(|(_, x)| x.clone())
                .collect();

            // Find the method
            match (tx.data.contract, tx.data.method) {
                // system interface
                // everyone has the same reallocate
                (_, 0) => system_0_realloc(&tx, &mut call_pages),
                (_, 1) => system_1_assign(&tx, &mut call_pages),
                // contract methods
                (DEFAULT_CONTRACT, DEFAULT_CONTRACT_MOVE_FUNDS) => {
                    default_contract_128_move_funds(&tx, &mut call_pages)
                }
                (DEFAULT_CONTRACT, BANK_PROCESS_TRANSACTION_METHOD) => {
                    Bank::default_contract_129_process_transaction(&tx, &mut call_pages)
                }
                #[cfg(test)]
                (DEFAULT_CONTRACT, 254) => {
                    default_contract_254_create_new_other(&tx, &mut call_pages)
                }
                #[cfg(test)]
                (DEFAULT_CONTRACT, 253) => {
                    default_contract_253_create_new_mine(&tx, &mut call_pages)
                }
                (contract, method) => {
                    warn!("unknown contract and method {:?} {:x}", contract, method)
                }
            };

            // TODO(anatoly): Verify Memory
            // Pages owned by the contract are Read/Write,
            // pages not owned by the contract are
            // Read only.  Code should verify memory integrity or
            // verify contract bytecode.

            // verify tokens
            if !Self::validate_balances(&tx, &loaded_pages, &call_pages) {
                return;
            }
            // write pages back to memory
            for (pre, post) in loaded_pages.iter_mut().zip(call_pages.into_iter()) {
                *pre = post;
            }
        });
    }

    /// parallel execution of contracts
    /// first we load the pages, then we pass all the pages to `execute` function which can
    /// safely call them all in parallel
    pub fn commit(
        &self,
        packet: &Vec<Call>,
        commits: &Vec<bool>,
        pages: &Vec<Vec<Option<usize>>>,
        loaded_page_table: &Vec<Vec<Page>>,
    ) {
        let mut page_table = self.page_table.write().unwrap();
        let mut count = 0;
        for (i, tx) in packet.into_iter().enumerate() {
            if !commits[i] {
                continue;
            }
            for (j, (_, oix)) in tx.data.keys.iter().zip(pages[i].iter()).enumerate() {
                let ix = oix.expect("checked pages should be loadable");
                page_table[ix] = loaded_page_table[i][j].clone();
            }
            count += 1;
        }
        self.transaction_count.fetch_add(count, Ordering::Relaxed);
    }
    pub fn commit_release_with_ctx(&self, packet: &Vec<Call>, ctx: &Context) {
        self.commit(packet, &ctx.commit, &ctx.pages, &ctx.loaded_page_table);
        //TODO(anatoly): generate blobs here
        self.release_memory_lock(packet, &ctx.lock);
    }

    pub fn get_balance(&self, key: &PublicKey) -> Option<i64> {
        let ap = self.allocated_pages.read().unwrap();
        let pt = self.page_table.read().unwrap();
        ap.lookup(key).map(|dx| {
            let page = &pt[dx];
            match page.contract {
                DEFAULT_CONTRACT => Bank::default_contract_get_balance(page),
                contract => {
                    warn!("get_balance: unknown contract {:?}", contract);
                    0
                }
            }
        })
    }
    pub fn get_version(&self, key: &PublicKey) -> Option<(u64, Signature)> {
        let ap = self.allocated_pages.read().unwrap();
        let pt = self.page_table.read().unwrap();
        ap.lookup(key).map(|dx| (pt[dx].version, pt[dx].signature))
    }
    pub fn release_memory_lock(&self, packet: &Vec<Call>, lock: &Vec<bool>) {
        //holds mem_locks mutex
        let mut mem_locks = self.mem_locks.lock().unwrap();
        for (i, call) in packet.iter().enumerate() {
            if !lock[i] {
                continue;
            }
            for key in &call.data.keys {
                mem_locks.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use bincode::deserialize;
    use logger;
    use packet::Recycler;
    use page_table::{Call, Context, Page, PageTable, K, N};
    use rayon::prelude::*;
    use signature::Signature;
    use std::collections::VecDeque;
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread::spawn;
    use std::time::Instant;

    #[test]
    fn mem_lock() {
        logger::setup();
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut lock2 = vec![false; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        for x in &lock {
            assert!(*x);
        }
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock2);
        for x in &lock2 {
            assert!(!*x);
        }
        pt.release_memory_lock(&transactions, &lock);

        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock2);
        for x in &lock2 {
            assert!(*x);
        }
        pt.release_memory_lock(&transactions, &lock2);
    }
    #[test]
    fn mem_lock_invalid_ledger() {
        logger::setup();
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let invalid_ledger = vec![false; N];
        let mut lock = vec![false; N];
        pt.acquire_memory_lock(&transactions, &invalid_ledger, &mut lock);
        for x in &lock {
            assert!(!*x);
        }
    }
    #[test]
    fn validate_call_miss() {
        let pt = PageTable::default();
        let fill_table: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&fill_table, true, 1_000_000);
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut checked = vec![false; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        for x in &checked {
            assert!(!x);
        }
    }
    #[test]
    fn validate_call_hit() {
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut checked = vec![false; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        for x in &checked {
            assert!(x);
        }
    }
    #[test]
    fn validate_call_low_version() {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        pt.bump_versions(&transactions);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut checked = vec![false; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        for tx in &mut transactions {
            tx.data.version = 0;
        }
        pt.validate_call(&transactions, &lock, &mut checked);
        for x in &checked {
            assert!(!x);
        }
    }
    #[test]
    fn find_new_keys_needs_alloc() {
        logger::setup();
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut needs_alloc = vec![false; N];
        let mut checked = vec![false; N];
        let mut pages = vec![vec![None; K]; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        pt.find_new_keys(&transactions, &checked, &mut needs_alloc, &mut pages);
        for x in &needs_alloc {
            assert!(x);
        }
    }
    #[test]
    fn find_new_keys_no_alloc() {
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        pt.force_allocate(&transactions, false, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut needs_alloc = vec![false; N];
        let mut checked = vec![false; N];
        let mut pages = vec![vec![None; K]; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        pt.find_new_keys(&transactions, &checked, &mut needs_alloc, &mut pages);
        for x in &needs_alloc {
            assert!(!x);
        }
    }
    #[test]
    fn allocate_new_keys_some_new_allocs() {
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut needs_alloc = vec![false; N];
        let mut checked = vec![false; N];
        let mut pages = vec![vec![None; K]; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        pt.find_new_keys(&transactions, &checked, &mut needs_alloc, &mut pages);
        pt.allocate_keys(&transactions, &checked, &needs_alloc, &mut pages);
        for (i, tx) in transactions.iter().enumerate() {
            for (j, _) in tx.data.keys.iter().enumerate() {
                assert!(pages[i][j].is_some());
            }
        }
    }
    #[test]
    fn allocate_new_keys_no_new_allocs() {
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        pt.force_allocate(&transactions, false, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut needs_alloc = vec![false; N];
        let mut checked = vec![false; N];
        let mut pages = vec![vec![None; K]; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        pt.find_new_keys(&transactions, &checked, &mut needs_alloc, &mut pages);
        for a in &needs_alloc {
            assert!(!a);
        }
        for (i, tx) in transactions.iter().enumerate() {
            for (j, _) in tx.data.keys.iter().enumerate() {
                assert!(pages[i][j].is_some());
            }
        }
        pt.allocate_keys(&transactions, &checked, &needs_alloc, &mut pages);
        for (i, tx) in transactions.iter().enumerate() {
            for (j, _) in tx.data.keys.iter().enumerate() {
                assert!(pages[i][j].is_some());
            }
        }
    }
    #[test]
    fn load_and_execute() {
        logger::setup();
        let pt = PageTable::default();
        let transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 10);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut needs_alloc = vec![false; N];
        let mut checked = vec![false; N];
        let mut pages = vec![vec![None; K]; N];
        let mut loaded_page_table: Vec<Vec<_>> = vec![vec![Page::default(); K]; N];
        let mut commit = vec![false; N];
        pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
        pt.validate_call(&transactions, &lock, &mut checked);
        pt.find_new_keys(&transactions, &checked, &mut needs_alloc, &mut pages);
        pt.sanity_check_pages(&transactions, &checked, &pages);
        pt.allocate_keys(&transactions, &checked, &needs_alloc, &mut pages);
        pt.sanity_check_pages(&transactions, &checked, &pages);
        pt.load_pages(
            &transactions,
            &mut checked,
            &mut pages,
            &mut loaded_page_table,
        );
        for c in &checked {
            assert!(*c);
        }
        PageTable::execute(&transactions, &checked, &mut loaded_page_table, &mut commit);
        pt.commit(&transactions, &commit, &pages, &loaded_page_table);
        pt.release_memory_lock(&transactions, &lock);
        for (i, x) in transactions.iter().enumerate() {
            assert!(checked[i]);
            let amount: i64 = deserialize(&x.data.user_data).unwrap();
            assert_eq!(pt.get_balance(&x.data.keys[1]), Some(amount));
            assert_eq!(
                pt.get_version(&x.data.keys[1]),
                Some((0, Signature::default()))
            );
            assert_eq!(
                pt.get_version(&x.data.keys[0]),
                Some((x.data.version + 1, x.proofs[0]))
            );
            assert_eq!(
                pt.get_balance(&x.data.keys[0]),
                Some(10 - (amount + x.data.fee))
            );
        }
    }
    /// catch indexing bugs that depend on context == packets in batch
    #[test]
    fn load_and_execute_variable_batches() {
        let pt = PageTable::default();
        let mut ctx = Context::default();
        let mut count = 0;
        for n in &[10, 9, 11, 8] {
            let mut txs: Vec<_> = (0..*n).map(|_r| Call::random_tx()).collect();
            let start_bal = 1_000_000;
            pt.force_allocate(&txs, true, start_bal);
            for is_valid in &mut ctx.valid_ledger {
                *is_valid = true;
            }
            pt.acquire_validate_find(&txs, &mut ctx);
            pt.allocate_keys_with_ctx(&txs, &mut ctx);
            pt.load_pages_with_ctx(&txs, &mut ctx);
            PageTable::execute_with_ctx(&txs, &mut ctx);
            pt.commit_release_with_ctx(&txs, &ctx);
            for t in &txs {
                assert_eq!(pt.get_balance(&t.data.keys[1]), Some(1));
                let amount: i64 = deserialize(&t.data.user_data).unwrap();
                assert_eq!(
                    pt.get_balance(&t.data.keys[0]),
                    Some(start_bal - (amount + t.data.fee))
                );
            }
            count += n;
            assert_eq!(pt.transaction_count(), count);
        }
    }
    #[test]
    fn load_and_execute_double_spends() {
        let pt = PageTable::default();
        let mut txs: Vec<_> = (0..2).map(|_r| Call::random_tx()).collect();
        let start_bal = 1_000_000;
        pt.force_allocate(&txs, true, start_bal);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        txs[0].data.method = 254;
        txs[1].data.method = 253;
        txs[0].data.fee = 3;
        txs[1].data.fee = 4;
        pt.acquire_validate_find(&txs, &mut ctx);
        pt.allocate_keys_with_ctx(&txs, &mut ctx);
        pt.load_pages_with_ctx(&txs, &mut ctx);
        PageTable::execute_with_ctx(&txs, &mut ctx);
        pt.commit_release_with_ctx(&txs, &ctx);
        assert_eq!(pt.get_balance(&txs[0].data.keys[1]), Some(0));
        assert_eq!(
            pt.get_balance(&txs[0].data.keys[0]),
            Some(start_bal - txs[0].data.fee)
        );
        assert_eq!(pt.get_balance(&txs[1].data.keys[1]), Some(0));
        assert_eq!(
            pt.get_balance(&txs[1].data.keys[0]),
            Some(start_bal - txs[1].data.fee)
        );
        //assert that a failed TX that spent fees increased the version
        assert_eq!(
            pt.get_version(&txs[0].data.keys[0]),
            Some((txs[0].data.version + 1, txs[0].proofs[0]))
        );
        assert_eq!(
            pt.get_version(&txs[1].data.keys[0]),
            Some((txs[1].data.version + 1, txs[1].proofs[0]))
        );
    }
    //TODO test assignment
    //TODO test spends of unasigned funds
    //TODO test realloc
    type ContextRecycler = Recycler<Context>;
    fn load_and_execute_pipeline_bench() {
        logger::setup();
        let context_recycler = ContextRecycler::default();
        let pt = PageTable::default();
        let count = 10000;
        let mut ttx: Vec<Vec<Call>> = (0..count)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for tx in &ttx {
            pt.force_allocate(tx, true, 1_000_000);
        }
        let (send_transactions, recv_transactions) = channel();
        let (send_execute, recv_execute) = channel();
        let (send_commit, recv_commit) = channel();
        let (send_answer, recv_answer) = channel();
        let spt = Arc::new(pt);

        let _reader = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for transactions in recv_transactions.iter() {
                    let transactions: Vec<Call> = transactions;
                    let octx = recycler.allocate();
                    {
                        let mut ctx = octx.write().unwrap();
                        for is_valid in &mut ctx.valid_ledger {
                            *is_valid = true;
                        }
                        lpt.acquire_validate_find(&transactions, &mut ctx);
                        lpt.allocate_keys_with_ctx(&transactions, &mut ctx);
                        lpt.load_pages_with_ctx(&transactions, &mut ctx);
                    }
                    send_execute.send((transactions, octx)).unwrap();
                }
            })
        };
        let _executor = {
            spawn(move || {
                for (transactions, octx) in recv_execute.iter() {
                    {
                        let mut ctx = octx.write().unwrap();
                        PageTable::execute_with_ctx(&transactions, &mut ctx);
                    }
                    send_commit.send((transactions, octx)).unwrap();
                }
            })
        };
        let _commiter = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for (transactions, octx) in recv_commit.iter() {
                    {
                        let ctx = octx.read().unwrap();
                        lpt.commit_release_with_ctx(&transactions, &ctx);
                        send_answer.send(()).unwrap();
                    }
                    recycler.recycle(octx);
                }
            })
        };
        //warmup
        let tt = ttx.pop().unwrap();
        send_transactions.send(tt).unwrap();
        recv_answer.recv().unwrap();

        let start = Instant::now();
        for _ in 1..count {
            let tt = ttx.pop().unwrap();
            send_transactions.send(tt).unwrap();
        }
        for _ in 1..count {
            recv_answer.recv().unwrap();
        }
        let done = start.elapsed();
        let ns = done.as_secs() as usize * 1_000_000_000 + done.subsec_nanos() as usize;
        let total = count * N;
        println!(
            "PIPELINE: done {:?} {}ns/packet {}ns/t {} tp/s",
            done,
            ns / (count - 1),
            ns / total,
            (1_000_000_000 * total) / ns
        );
    }
    fn load_and_execute_par_pipeline_bench() {
        logger::setup();
        let context_recycler = ContextRecycler::default();
        let pt = PageTable::default();
        let count = 10000;
        let mut ttx: Vec<Vec<Call>> = (0..count)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for tx in &ttx {
            pt.force_allocate(tx, true, 1_000_000);
        }
        let (send_transactions, recv_transactions) = channel();
        let (send_execute, recv_execute) = channel();
        let (send_commit, recv_commit) = channel();
        let (send_answer, recv_answer) = channel();
        let spt = Arc::new(pt);

        let _reader = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for transactions in recv_transactions.iter() {
                    let transactions: Vec<Call> = transactions;
                    let octx = recycler.allocate();
                    {
                        let mut ctx = octx.write().unwrap();
                        for is_valid in &mut ctx.valid_ledger {
                            *is_valid = true;
                        }
                        lpt.acquire_validate_find(&transactions, &mut ctx);
                        lpt.allocate_keys_with_ctx(&transactions, &mut ctx);
                        lpt.load_pages_with_ctx(&transactions, &mut ctx);
                    }
                    send_execute.send((transactions, octx)).unwrap();
                }
            })
        };
        let _executor = {
            spawn(move || {
                while let Ok(tx) = recv_execute.recv() {
                    let mut events = VecDeque::new();
                    events.push_back(tx);
                    while let Ok(more) = recv_execute.try_recv() {
                        events.push_back(more);
                    }
                    events.par_iter().for_each(|(transactions, octx)| {
                        let mut ctx = octx.write().unwrap();
                        PageTable::execute_with_ctx(&transactions, &mut ctx);
                    });
                    send_commit.send(events).unwrap();
                }
            })
        };
        let _commiter = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for mut events in recv_commit.iter() {
                    for (transactions, octx) in &events {
                        let ctx = octx.read().unwrap();
                        lpt.commit_release_with_ctx(transactions, &ctx);
                    }
                    send_answer.send(events.len()).unwrap();
                    while let Some((_, octx)) = events.pop_front() {
                        recycler.recycle(octx);
                    }
                }
            })
        };
        let _sender = {
            spawn(move || {
                while let Some(tt) = ttx.pop() {
                    let _ = send_transactions.send(tt);
                }
            })
        };
        //warmup
        recv_answer.recv().unwrap();
        let start = Instant::now();
        let mut total = 1;
        while total < count {
            total += recv_answer.recv().unwrap();
        }
        let done = start.elapsed();
        let ns = done.as_secs() as usize * 1_000_000_000 + done.subsec_nanos() as usize;
        let total = count * N;
        println!(
            "PAR_PIPELINE: done {:?} {}ns/packet {}ns/t {} tp/s",
            done,
            ns / (count - 1),
            ns / total,
            (1_000_000_000 * total) / ns
        );
    }
    pub fn load_and_execute_mt_bench(max_threads: usize) {
        let pt = PageTable::default();
        let count = 10000;
        let mut ttx: Vec<Vec<Call>> = (0..count)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for tx in &ttx {
            pt.force_allocate(tx, true, 1_000_000);
        }
        let (send_answer, recv_answer) = channel();
        let spt = Arc::new(pt);
        let threads: Vec<_> = (0..max_threads)
            .map(|_| {
                let (send, recv) = channel();
                let response = send_answer.clone();
                let lpt = spt.clone();
                let t = spawn(move || {
                    let mut ctx = Context::default();
                    for is_valid in &mut ctx.valid_ledger {
                        *is_valid = true;
                    }
                    for transactions in recv.iter() {
                        let transactions: Vec<Call> = transactions;
                        lpt.acquire_validate_find(&transactions, &mut ctx);
                        lpt.allocate_keys_with_ctx(&transactions, &mut ctx);
                        lpt.load_pages_with_ctx(&transactions, &mut ctx);
                        PageTable::execute_with_ctx(&transactions, &mut ctx);
                        lpt.commit_release_with_ctx(&transactions, &ctx);
                        response.send(()).unwrap();
                    }
                });
                (t, send)
            })
            .collect();
        let _sender = {
            spawn(move || {
                for thread in 0..count {
                    let tt = ttx.pop().unwrap();
                    threads[thread % max_threads].1.send(tt).unwrap();
                }
            })
        };
        //warmup
        for _ in 0..max_threads {
            recv_answer.recv().unwrap();
        }

        let start = Instant::now();
        for _thread in max_threads..count {
            recv_answer.recv().unwrap();
        }
        let done = start.elapsed();
        let ns = done.as_secs() as usize * 1_000_000_000 + done.subsec_nanos() as usize;
        let total = (count - max_threads) * N;
        println!(
            "MT-{}: done {:?} {}ns/packet {}ns/t {} tp/s",
            max_threads,
            done,
            ns / (count - max_threads),
            ns / total,
            (1_000_000_000 * total) / ns
        );
    }
    #[test]
    #[ignore]
    fn load_and_execute_benches() {
        println!("load_and_execute_mt_bench(1)");
        load_and_execute_mt_bench(1);
        println!("load_and_execute_mt_bench(2)");
        load_and_execute_mt_bench(2);
        println!("load_and_execute_mt_bench(3)");
        load_and_execute_mt_bench(3);
        println!("load_and_execute_mt_bench(4)");
        load_and_execute_mt_bench(4);
        println!("load_and_execute_mt_bench(8)");
        load_and_execute_mt_bench(8);
        println!("load_and_execute_mt_bench(16)");
        load_and_execute_mt_bench(16);
        println!("load_and_execute_mt_bench(32)");
        load_and_execute_mt_bench(32);
        println!("load_and_execute_pipeline_bench");
        load_and_execute_pipeline_bench();
        println!("load_and_execute_par_pipeline_bench");
        load_and_execute_par_pipeline_bench();
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use packet::Recycler;
    use page_table::{self, Call, Context, PageTable, N};
    use rand::{thread_rng, RngCore};
    use signature::Signature;
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread::spawn;

    #[bench]
    fn update_version_baseline(bencher: &mut Bencher) {
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
        });
    }
    #[bench]
    fn mem_lock(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
            pt.release_memory_lock(&transactions, &lock);
        });
    }
    #[bench]
    fn mem_lock_invalid(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let valid_ledger = vec![false; N];
        let mut lock = vec![false; N];
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
            pt.release_memory_lock(&transactions, &lock);
        });
    }
    #[bench]
    fn validate_call_miss(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let fill_table: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&fill_table, true, 1_000_000);
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut checked = vec![false; N];
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
            pt.validate_call(&transactions, &lock, &mut checked);
            pt.release_memory_lock(&transactions, &lock);
        });
    }
    #[bench]
    fn validate_call_hit(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let valid_ledger = vec![true; N];
        let mut lock = vec![false; N];
        let mut checked = vec![false; N];
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_memory_lock(&transactions, &valid_ledger, &mut lock);
            pt.validate_call(&transactions, &lock, &mut checked);
            pt.release_memory_lock(&transactions, &lock);
        });
    }
    #[bench]
    fn find_new_keys_needs_alloc(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.release_memory_lock(&transactions, &ctx.lock);
        });
    }
    #[bench]
    fn find_new_keys_no_alloc(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        pt.force_allocate(&transactions, false, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.release_memory_lock(&transactions, &ctx.lock);
        });
    }
    #[bench]
    fn allocate_new_keys_some_new_allocs(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }

            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            pt.release_memory_lock(&transactions, &ctx.lock);
        });
    }
    #[bench]
    fn allocate_new_keys_no_new_allocs(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        pt.force_allocate(&transactions, false, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            pt.release_memory_lock(&transactions, &ctx.lock);
        });
    }
    #[bench]
    fn load_pages(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            pt.load_pages_with_ctx(&transactions, &mut ctx);
            pt.release_memory_lock(&transactions, &ctx.lock);
        });
    }
    #[bench]
    fn load_and_execute(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut transactions: Vec<_> = (0..N).map(|_r| Call::random_tx()).collect();
        pt.force_allocate(&transactions, true, 1_000_000);
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            for tx in &mut transactions {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            pt.load_pages_with_ctx(&transactions, &mut ctx);
            PageTable::execute_with_ctx(&transactions, &mut ctx);
            pt.commit_release_with_ctx(&transactions, &ctx);
        });
    }

    #[bench]
    fn load_and_execute_large_table(bencher: &mut Bencher) {
        let pt = PageTable::default();
        let mut ttx: Vec<Vec<_>> = (0..N)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for transactions in &ttx {
            pt.force_allocate(transactions, true, 1_000_000);
        }
        page_table::test::load_and_execute();
        let mut ctx = Context::default();
        for is_valid in &mut ctx.valid_ledger {
            *is_valid = true;
        }
        bencher.iter(move || {
            let transactions = &mut ttx[thread_rng().next_u64() as usize % N];
            for tx in transactions.iter_mut() {
                tx.data.version += 1;
            }
            pt.acquire_validate_find(&transactions, &mut ctx);
            pt.allocate_keys_with_ctx(&transactions, &mut ctx);
            pt.load_pages_with_ctx(&transactions, &mut ctx);
            PageTable::execute_with_ctx(&transactions, &mut ctx);
            pt.commit_release_with_ctx(&transactions, &ctx);
        });
    }
    #[bench]
    fn load_and_execute_mt3_experimental(bencher: &mut Bencher) {
        const T: usize = 3;
        let pt = PageTable::default();
        let count = 10000;
        let mut ttx: Vec<Vec<Call>> = (0..count)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for tx in &ttx {
            pt.force_allocate(tx, true, 1_000_000);
        }
        let (send_answer, recv_answer) = channel();
        let spt = Arc::new(pt);
        let threads: Vec<_> = (0..T)
            .map(|_| {
                let (send, recv) = channel();
                let response = send_answer.clone();
                let lpt = spt.clone();
                let t = spawn(move || {
                    let mut ctx = Context::default();
                    for is_valid in &mut ctx.valid_ledger {
                        *is_valid = true;
                    }
                    for transactions in recv.iter() {
                        lpt.acquire_validate_find(&transactions, &mut ctx);
                        lpt.allocate_keys_with_ctx(&transactions, &mut ctx);
                        lpt.load_pages_with_ctx(&transactions, &mut ctx);
                        PageTable::execute_with_ctx(&transactions, &mut ctx);
                        lpt.commit_release_with_ctx(&transactions, &ctx);
                        if response.send(()).is_err() {
                            return;
                        }
                    }
                });
                (t, send)
            })
            .collect();
        let _sender = {
            spawn(move || {
                for thread in 0..count {
                    let tt = ttx.pop().unwrap();
                    if threads[thread % T].1.send(tt).is_err() {
                        return;
                    }
                }
            })
        };

        //warmup
        for _ in 0..T {
            recv_answer.recv().unwrap();
        }
        bencher.iter(move || {
            recv_answer.recv().unwrap();
        });
    }
    type ContextRecycler = Recycler<Context>;
    #[bench]
    fn load_and_execute_pipeline_experimental(bencher: &mut Bencher) {
        let context_recycler = ContextRecycler::default();
        let pt = PageTable::default();
        let count = 10000;
        let mut ttx: Vec<Vec<Call>> = (0..count)
            .map(|_| (0..N).map(|_r| Call::random_tx()).collect())
            .collect();
        for tx in &ttx {
            pt.force_allocate(tx, true, 1_000_000);
        }
        let (send_transactions, recv_transactions) = channel();
        let (send_execute, recv_execute) = channel();
        let (send_commit, recv_commit) = channel();
        let (send_answer, recv_answer) = channel();
        let spt = Arc::new(pt);

        let _reader = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for transactions in recv_transactions.iter() {
                    let octx = recycler.allocate();
                    {
                        let mut ctx = octx.write().unwrap();

                        for is_valid in &mut ctx.valid_ledger {
                            *is_valid = true;
                        }
                        lpt.acquire_validate_find(&transactions, &mut ctx);
                        lpt.allocate_keys_with_ctx(&transactions, &mut ctx);
                        lpt.load_pages_with_ctx(&transactions, &mut ctx);
                    }
                    if send_execute.send((transactions, octx)).is_err() {
                        return;
                    }
                }
            })
        };
        let _executor = {
            spawn(move || {
                for (transactions, octx) in recv_execute.iter() {
                    {
                        let mut ctx = octx.write().unwrap();
                        PageTable::execute_with_ctx(&transactions, &mut ctx);
                    }
                    if send_commit.send((transactions, octx)).is_err() {
                        return;
                    }
                }
            })
        };
        let _commiter = {
            let lpt = spt.clone();
            let recycler = context_recycler.clone();
            spawn(move || {
                for (transactions, octx) in recv_commit.iter() {
                    {
                        let ctx = octx.read().unwrap();
                        lpt.commit_release_with_ctx(&transactions, &ctx);
                        if send_answer.send(()).is_err() {
                            return;
                        }
                    }
                    recycler.recycle(octx);
                }
            })
        };
        let _sender = {
            spawn(move || {
                while let Some(tt) = ttx.pop() {
                    if send_transactions.send(tt).is_err() {
                        return;
                    }
                }
            })
        };
        //warmup
        recv_answer.recv().unwrap();
        bencher.iter(move || {
            recv_answer.recv().unwrap();
        });
    }
}
