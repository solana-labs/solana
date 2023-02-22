use {
    crate::storable_accounts::StorableAccounts,
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        hash::Hash,
        pubkey::Pubkey,
        stake_history::Epoch,
    },
    std::{borrow::Borrow, marker::PhantomData},
};

pub type StoredMetaWriteVersion = u64;

/// Goal is to eliminate copies and data reshaping given various code paths that store accounts.
/// This struct contains what is needed to store accounts to a storage
/// 1. account & pubkey (StorableAccounts)
/// 2. hash per account (Maybe in StorableAccounts, otherwise has to be passed in separately)
/// 3. write version per account (Maybe in StorableAccounts, otherwise has to be passed in separately)
pub struct StorableAccountsWithHashesAndWriteVersions<
    'a: 'b,
    'b,
    T: ReadableAccount + Sync + 'b,
    U: StorableAccounts<'a, T>,
    V: Borrow<Hash>,
> {
    /// accounts to store
    /// always has pubkey and account
    /// may also have hash and write_version per account
    pub(crate) accounts: &'b U,
    /// if accounts does not have hash and write version, this has a hash and write version per account
    hashes_and_write_versions: Option<(Vec<V>, Vec<StoredMetaWriteVersion>)>,
    _phantom: PhantomData<&'a T>,
}

impl<'a: 'b, 'b, T: ReadableAccount + Sync + 'b, U: StorableAccounts<'a, T>, V: Borrow<Hash>>
    StorableAccountsWithHashesAndWriteVersions<'a, 'b, T, U, V>
{
    /// used when accounts contains hash and write version already
    pub fn new(accounts: &'b U) -> Self {
        assert!(accounts.has_hash_and_write_version());
        Self {
            accounts,
            hashes_and_write_versions: None,
            _phantom: PhantomData,
        }
    }
    /// used when accounts does NOT contains hash or write version
    /// In this case, hashes and write_versions have to be passed in separately and zipped together.
    pub fn new_with_hashes_and_write_versions(
        accounts: &'b U,
        hashes: Vec<V>,
        write_versions: Vec<StoredMetaWriteVersion>,
    ) -> Self {
        assert!(!accounts.has_hash_and_write_version());
        assert_eq!(accounts.len(), hashes.len());
        assert_eq!(write_versions.len(), hashes.len());
        Self {
            accounts,
            hashes_and_write_versions: Some((hashes, write_versions)),
            _phantom: PhantomData,
        }
    }

    /// get all account fields at 'index'
    pub fn get(&self, index: usize) -> (Option<&T>, &Pubkey, &Hash, StoredMetaWriteVersion) {
        let account = self.accounts.account_default_if_zero_lamport(index);
        let pubkey = self.accounts.pubkey(index);
        let (hash, write_version) = if self.accounts.has_hash_and_write_version() {
            (
                self.accounts.hash(index),
                self.accounts.write_version(index),
            )
        } else {
            let item = self.hashes_and_write_versions.as_ref().unwrap();
            (item.0[index].borrow(), item.1[index])
        };
        (account, pubkey, hash, write_version)
    }

    /// None if account at index has lamports == 0
    /// Otherwise, Some(account)
    /// This is the only way to access the account.
    pub fn account(&self, index: usize) -> Option<&T> {
        self.accounts.account_default_if_zero_lamport(index)
    }

    /// # accounts to write
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// References to account data stored elsewhere. Getting an `Account` requires cloning
/// (see `StoredAccountMeta::clone_account()`).
#[derive(PartialEq, Eq, Debug)]
pub struct StoredAccountMeta<'a> {
    pub(crate) meta: &'a StoredMeta,
    /// account data
    pub(crate) account_meta: &'a AccountMeta,
    pub(crate) data: &'a [u8],
    pub(crate) offset: usize,
    pub(crate) stored_size: usize,
    pub(crate) hash: &'a Hash,
}

impl<'a> StoredAccountMeta<'a> {
    /// Return a new Account by copying all the data referenced by the `StoredAccountMeta`.
    pub fn clone_account(&self) -> AccountSharedData {
        AccountSharedData::from(Account {
            lamports: self.account_meta.lamports,
            owner: self.account_meta.owner,
            executable: self.account_meta.executable,
            rent_epoch: self.account_meta.rent_epoch,
            data: self.data.to_vec(),
        })
    }

    pub fn pubkey(&self) -> &Pubkey {
        &self.meta.pubkey
    }

    pub fn hash(&self) -> &Hash {
        self.hash
    }

    pub fn stored_size(&self) -> usize {
        self.stored_size
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_len(&self) -> u64 {
        self.meta.data_len
    }

    pub fn write_version(&self) -> StoredMetaWriteVersion {
        self.meta.write_version_obsolete
    }

    pub(crate) fn sanitize(&self) -> bool {
        self.sanitize_executable() && self.sanitize_lamports()
    }

    pub(crate) fn sanitize_executable(&self) -> bool {
        // Sanitize executable to ensure higher 7-bits are cleared correctly.
        self.ref_executable_byte() & !1 == 0
    }

    pub(crate) fn sanitize_lamports(&self) -> bool {
        // Sanitize 0 lamports to ensure to be same as AccountSharedData::default()
        self.account_meta.lamports != 0 || self.clone_account() == AccountSharedData::default()
    }

    pub(crate) fn ref_executable_byte(&self) -> &u8 {
        // Use extra references to avoid value silently clamped to 1 (=true) and 0 (=false)
        // Yes, this really happens; see test_new_from_file_crafted_executable
        let executable_bool: &bool = &self.account_meta.executable;
        // UNSAFE: Force to interpret mmap-backed bool as u8 to really read the actual memory content
        let executable_byte: &u8 = unsafe { &*(executable_bool as *const bool as *const u8) };
        executable_byte
    }
}

impl<'a> ReadableAccount for StoredAccountMeta<'a> {
    fn lamports(&self) -> u64 {
        self.account_meta.lamports
    }
    fn data(&self) -> &[u8] {
        self.data()
    }
    fn owner(&self) -> &Pubkey {
        &self.account_meta.owner
    }
    fn executable(&self) -> bool {
        self.account_meta.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.account_meta.rent_epoch
    }
}

/// Meta contains enough context to recover the index from storage itself
/// This struct will be backed by mmaped and snapshotted data files.
/// So the data layout must be stable and consistent across the entire cluster!
#[derive(Clone, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct StoredMeta {
    /// global write version
    /// This will be made completely obsolete such that we stop storing it.
    /// We will not support multiple append vecs per slot anymore, so this concept is no longer necessary.
    /// Order of stores of an account to an append vec will determine 'latest' account data per pubkey.
    pub write_version_obsolete: StoredMetaWriteVersion,
    pub data_len: u64,
    /// key for the account
    pub pubkey: Pubkey,
}

/// This struct will be backed by mmaped and snapshotted data files.
/// So the data layout must be stable and consistent across the entire cluster!
#[derive(Serialize, Deserialize, Clone, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct AccountMeta {
    /// lamports in the account
    pub lamports: u64,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: Epoch,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
}

impl<'a, T: ReadableAccount> From<&'a T> for AccountMeta {
    fn from(account: &'a T) -> Self {
        Self {
            lamports: account.lamports(),
            owner: *account.owner(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
        }
    }
}

impl<'a, T: ReadableAccount> From<Option<&'a T>> for AccountMeta {
    fn from(account: Option<&'a T>) -> Self {
        match account {
            Some(account) => AccountMeta::from(account),
            None => AccountMeta::default(),
        }
    }
}
