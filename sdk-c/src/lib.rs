use bincode::{deserialize, serialize};
use libc::{c_int, size_t};
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use solana_ed25519_dalek::{SignatureError, KEYPAIR_LENGTH, PUBLIC_KEY_LENGTH};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::CompiledInstruction as CompiledInstructionNative;
use solana_sdk::message::Message as MessageNative;
use solana_sdk::message::MessageHeader as MessageHeaderNative;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature as SignatureNative;
use solana_sdk::signature::{Keypair as KeypairNative, KeypairUtil};
use solana_sdk::transaction::Transaction as TransactionNative;
use std::convert::TryInto;
use std::ffi::CString;
use std::os::raw::c_char;
use std::vec::Vec;
use std::{fmt, mem, ptr, slice};

#[repr(C)]
#[derive(Debug)]
pub struct Transaction {
    /// A set of digital signatures of `account_keys`, `program_ids`, `recent_blockhash`, and `instructions`, signed by the first
    /// signatures_len keys of account_keys
    pub signatures: CVec<Signature>,

    /// The message to sign.
    pub message: Message,
}

impl Transaction {
    pub fn from_native(mut t: TransactionNative) -> Self {
        t.signatures.shrink_to_fit();
        Self {
            signatures: CVec::from_native(
                t.signatures
                    .into_iter()
                    .map(Signature::from_native)
                    .collect(),
            ),
            message: Message::from_native(t.message),
        }
    }

    pub unsafe fn into_native(self) -> TransactionNative {
        TransactionNative {
            signatures: CVec::into_native(self.signatures)
                .iter()
                .map(|s| s.new_native())
                .collect(),
            message: self.message.into_native(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub unsafe fn clone(&self) -> Self {
        Self {
            signatures: self.signatures.clone(),
            message: self.message.clone(),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Message {
    /// The message header, identifying signed and read-only `account_keys`
    pub header: MessageHeader,

    /// All the account keys used by this transaction
    pub account_keys: CVec<Pubkey>,

    /// The id of a recent ledger entry.
    pub recent_blockhash: Hash,

    /// Programs that will be executed in sequence and committed in one atomic transaction if all
    /// succeed.
    pub instructions: CVec<CompiledInstruction>,
}

impl Message {
    pub fn from_native(m: MessageNative) -> Self {
        Self {
            header: MessageHeader::from_native(m.header),
            account_keys: CVec::from_native(m.account_keys),
            recent_blockhash: m.recent_blockhash,
            instructions: CVec::from_native(
                m.instructions
                    .into_iter()
                    .map(CompiledInstruction::from_native)
                    .collect(),
            ),
        }
    }

    pub unsafe fn into_native(self) -> MessageNative {
        MessageNative {
            header: self.header.into_native(),
            account_keys: CVec::into_native(self.account_keys),
            recent_blockhash: self.recent_blockhash,
            instructions: CVec::into_native(self.instructions)
                .into_iter()
                .map(|i| i.into_native())
                .collect(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub unsafe fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            account_keys: self.account_keys.clone(),
            recent_blockhash: self.recent_blockhash,
            instructions: self.instructions.clone(),
        }
    }
}

/// An instruction to execute a program
#[repr(C)]
#[derive(Debug)]
pub struct CompiledInstruction {
    /// Index into the transaction keys array indicating the program account that executes this instruction
    pub program_id_index: u8,

    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program
    pub accounts: CVec<u8>,

    /// The program input data
    pub data: CVec<u8>,
}

impl CompiledInstruction {
    pub fn from_native(c: CompiledInstructionNative) -> Self {
        Self {
            program_id_index: c.program_id_index,
            accounts: CVec::from_native(c.accounts),
            data: CVec::from_native(c.data),
        }
    }

    pub unsafe fn into_native(self) -> CompiledInstructionNative {
        CompiledInstructionNative {
            program_id_index: self.program_id_index,
            accounts: CVec::into_native(self.accounts),
            data: CVec::into_native(self.data),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub unsafe fn clone(&self) -> Self {
        Self {
            program_id_index: self.program_id_index,
            accounts: self.accounts.clone(),
            data: self.data.clone(),
        }
    }
}

#[repr(C)]
#[derive(Default, Debug, Clone)]
pub struct MessageHeader {
    /// The number of signatures required for this message to be considered valid. The
    /// signatures must match the first `num_required_signatures` of `account_keys`.
    pub num_required_signatures: u8,

    /// The last num_readonly_signed_accounts of the signed keys are read-only accounts. Programs
    /// may process multiple transactions that load read-only accounts within a single PoH entry,
    /// but are not permitted to credit or debit lamports or modify account data. Transactions
    /// targeting the same read-write account are evaluated sequentially.
    pub num_readonly_signed_accounts: u8,

    /// The last num_readonly_unsigned_accounts of the unsigned keys are read-only accounts.
    pub num_readonly_unsigned_accounts: u8,
}

impl MessageHeader {
    pub fn from_native(h: MessageHeaderNative) -> Self {
        Self {
            num_required_signatures: h.num_required_signatures,
            num_readonly_signed_accounts: h.num_readonly_signed_accounts,
            num_readonly_unsigned_accounts: h.num_readonly_unsigned_accounts,
        }
    }

    pub fn into_native(self) -> MessageHeaderNative {
        MessageHeaderNative {
            num_required_signatures: self.num_required_signatures,
            num_readonly_signed_accounts: self.num_readonly_signed_accounts,
            num_readonly_unsigned_accounts: self.num_readonly_unsigned_accounts,
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Signature([u8; 64]);

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(&self.0[..]).into_string())
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(&self.0[..]).into_string())
    }
}

impl Signature {
    pub fn from_native(s: SignatureNative) -> Self {
        Self(s.into())
    }

    pub fn new_native(&self) -> SignatureNative {
        SignatureNative::new(&self.0[..])
    }
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Keypair([u8; KEYPAIR_LENGTH]);

impl Keypair {
    pub fn from_native(k: &KeypairNative) -> Self {
        Self(k.to_bytes())
    }

    pub fn new_native(&self) -> Result<KeypairNative, SignatureError> {
        KeypairNative::from_bytes(&self.0[..])
    }
}

/// A representation of a Rust Vec that can be passed to C. Should not be modified or copied by C code.
#[repr(C)]
#[derive(Debug)]
pub struct CVec<T> {
    data: *mut T,
    len: size_t,
    capacity: size_t,
}

impl<T> CVec<T> {
    pub fn from_native(mut v: Vec<T>) -> Self {
        let out = Self {
            data: v.as_mut_ptr(),
            len: v.len(),
            capacity: v.capacity(),
        };
        mem::forget(v);
        out
    }

    pub unsafe fn into_native(self) -> Vec<T> {
        Vec::from_raw_parts(self.data, self.len, self.capacity)
    }
}

impl<T: Clone> CVec<T> {
    #[allow(clippy::should_implement_trait)]
    pub unsafe fn clone(&self) -> Self {
        let native = Vec::from_raw_parts(self.data, self.len, self.capacity);
        let mut new: Vec<T> = Vec::with_capacity(native.capacity());
        <Vec<T> as Clone>::clone_from(&mut new, &native);
        mem::forget(native);
        Self::from_native(new)
    }
}

impl CVec<CompiledInstruction> {
    #[allow(clippy::should_implement_trait)]
    pub unsafe fn clone(&self) -> Self {
        let native = Vec::from_raw_parts(self.data, self.len, self.capacity);
        let mut new: Vec<CompiledInstruction> = Vec::with_capacity(native.capacity());
        for elem in &native {
            new.push(elem.clone());
        }
        mem::forget(native);
        Self::from_native(new)
    }
}

#[no_mangle]
pub unsafe extern "C" fn free_transaction(tx: *mut Transaction) {
    Box::from_raw(tx);
}

#[no_mangle]
pub unsafe extern "C" fn free_message(m: *mut Message) {
    Box::from_raw(m);
}

#[no_mangle]
pub unsafe extern "C" fn free_message_header(mh: *mut MessageHeader) {
    Box::from_raw(mh);
}

#[no_mangle]
pub unsafe extern "C" fn free_signature(s: *mut Signature) {
    Box::from_raw(s);
}

#[no_mangle]
pub unsafe extern "C" fn free_compiled_instruction(i: *mut CompiledInstruction) {
    Box::from_raw(i);
}

#[no_mangle]
pub unsafe extern "C" fn free_c_string(s: *mut c_char) {
    CString::from_raw(s);
}

#[no_mangle]
pub unsafe extern "C" fn new_unsigned_transaction(message: *mut Message) -> *mut Transaction {
    let message = Box::from_raw(message);
    let tx = Box::new(Transaction::from_native(TransactionNative::new_unsigned(
        message.into_native(),
    )));
    Box::into_raw(tx)
}

/// Generate a `Keypair` from a uint8_t[32] seed. Assumes the pointer is to an array of length 32.
///
/// # Undefined Behavior
///
/// Causes UB if `seed` is not a pointer to an array of length 32 or if `seed` is `NULL`
#[no_mangle]
pub unsafe extern "C" fn generate_keypair(seed: *const u8) -> *mut Keypair {
    let seed = <&[u8] as TryInto<&[u8; PUBLIC_KEY_LENGTH]>>::try_into(slice::from_raw_parts(
        seed,
        PUBLIC_KEY_LENGTH,
    ))
    .unwrap(); // Guaranteed not to panic
    let mut rng = ChaChaRng::from_seed(*seed);
    let keypair = KeypairNative::generate(&mut rng);
    let keypair = Box::new(Keypair::from_native(&keypair));
    Box::into_raw(keypair)
}

/// Get a pubkey from a keypair. Returns `NULL` if the conversion causes an error
///
/// # Undefined Behavior
///
/// Causes UB if `keypair` is `NULL` or if `keypair` in not a pointer to a valid `Keypair`
#[no_mangle]
pub unsafe extern "C" fn get_keypair_pubkey(keypair: *const Keypair) -> *mut Pubkey {
    let keypair = if let Ok(k) = (*keypair).new_native() {
        k
    } else {
        return ptr::null_mut();
    };
    let pubkey = Box::new(keypair.pubkey());
    Box::into_raw(pubkey)
}

/// Serialize a `Transaction` and save a pointer to the resulting byte array to `serialized`, and its
/// length to `len`. Returns `0` for success, other for failure. Consumes and frees the `Transaction`.
///
/// # Undefined Behavior
///
/// Causes UB if any of the input pointers is `NULL`, or if `tx` is not a valid `Transaction`
#[no_mangle]
pub unsafe extern "C" fn serialize_transaction(
    tx: *mut Transaction,
    serialized: *mut *const u8,
    len: *mut size_t,
) -> c_int {
    let tx = Box::from_raw(tx);
    let tx = tx.into_native();
    let mut serialized_tx = if let Ok(s) = serialize(&tx) {
        s
    } else {
        return 1;
    };
    serialized_tx.shrink_to_fit();
    *serialized = serialized_tx.as_mut_ptr();
    *len = serialized_tx.len();
    mem::forget(serialized_tx);
    0
}

/// Deserialize an array of bytes into a `Transaction`. Returns `NULL` if deserialization fails
///
/// # Undefined Behavior
///
/// Causes UB if `bytes` is `NULL`, or if `bytes` does not point to a valid array of length `len`
#[no_mangle]
pub unsafe extern "C" fn deserialize_transaction(
    bytes: *const u8,
    len: size_t,
) -> *mut Transaction {
    let slice = slice::from_raw_parts(bytes, len);
    let tx = if let Ok(t) = deserialize::<TransactionNative>(slice) {
        t
    } else {
        return ptr::null_mut();
    };
    let tx = Box::new(Transaction::from_native(tx));
    Box::into_raw(tx)
}

/// Sign a transaction with a subset of the required `Keypair`s. If the `recent_blockhash` supplied
/// does not match that of the `Transaction`, the supplied one will be used, and any existing
/// signatures will be discarded. Returns `0` for success, other for failure.
///
/// # Undefined Behavior
///
/// Causes UB if any of the pointers is `NULL`, or if `keypairs` does not point to a valid array of
/// `Keypairs` of length `num_keypairs`
#[no_mangle]
pub unsafe extern "C" fn transaction_partial_sign(
    tx: *mut Transaction,
    keypairs: *const Keypair,
    num_keypairs: size_t,
    recent_blockhash: *const Hash,
) -> c_int {
    let mut tx = Box::from_raw(tx);
    let mut tx_native = tx.clone().into_native();

    let keypairs = slice::from_raw_parts(keypairs, num_keypairs);
    let keypairs: Vec<Result<_, _>> = keypairs.iter().map(|k| k.new_native()).collect();
    let keypairs: Vec<KeypairNative> = if keypairs.iter().all(|k| k.is_ok()) {
        keypairs
            .into_iter()
            .map(|k| k.expect("This shouldn't ever happen"))
            .collect()
    } else {
        return 1;
    };
    let keypairs_ref: Vec<&KeypairNative> = keypairs.iter().collect();

    let positions = if let Ok(v) = tx_native.get_signing_keypair_positions(&keypairs_ref[..]) {
        v
    } else {
        return 2;
    };
    let positions: Vec<usize> = if positions.iter().all(|pos| pos.is_some()) {
        positions
            .iter()
            .map(|pos| pos.expect("This shouldn't ever happen"))
            .collect()
    } else {
        return 3;
    };

    tx_native.partial_sign_unchecked(&keypairs_ref[..], positions, *recent_blockhash);
    *tx = Transaction::from_native(tx_native);
    Box::into_raw(tx);
    0
}

/// Get the printable c-string of a Pubkey. The returned c-string must be freed with `free_c_string()`
/// Returns `NULL` if the conversion fails.
///
/// # Undefined Behavior
///
/// Causes UB if `pubkey` is `NULL`, or if the returned c-string is freed by any method other than
/// calling `free_c_string()`
#[no_mangle]
pub unsafe extern "C" fn get_pubkey_string(pubkey: *const Pubkey) -> *mut c_char {
    if let Ok(s) = CString::new(format!("{}", *pubkey)) {
        s.into_raw()
    } else {
        ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use bincode::serialize;
    use rand_chacha::ChaChaRng;
    use rand_core::SeedableRng;
    use solana_sdk::signature::{Keypair as KeypairNative, KeypairUtil};
    use solana_sdk::system_transaction;

    #[test]
    fn test_generate_keypair() {
        let seed = [1u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);
        let keypair = KeypairNative::generate(&mut rng);
        let c_keypair = unsafe { Box::from_raw(generate_keypair(seed.as_ptr())) };
        assert_eq!(c_keypair.new_native(), Ok(keypair));
    }

    #[test]
    fn test_get_pubkey() {
        let seed = [1u8; 32];
        unsafe {
            let keypair = generate_keypair(seed.as_ptr());
            let pubkey = Box::from_raw(get_keypair_pubkey(keypair));
            let keypair = Box::from_raw(keypair);
            let keypair = keypair.new_native().unwrap();
            assert_eq!(keypair.pubkey(), *pubkey);
        };
    }

    #[test]
    fn test_serialize_transaction() {
        let key = KeypairNative::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, 50, blockhash);
        let serialized = serialize(&tx).unwrap();
        let tx = Box::new(Transaction::from_native(tx));
        let tx = Box::into_raw(tx);
        let mut res: *const u8 = ptr::null_mut();
        let mut len: size_t = 0;
        let slice;
        unsafe {
            assert_eq!(0, serialize_transaction(tx, &mut res, &mut len));
            slice = slice::from_raw_parts(res, len);
        }
        assert_eq!(serialized, slice);
    }

    #[test]
    fn test_deserialize_transaction() {
        let key = KeypairNative::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, 50, blockhash);
        let serialized = serialize(&tx).unwrap();
        let deserialized;
        unsafe {
            let ret = deserialize_transaction(serialized.as_ptr(), serialized.len());
            assert_ne!(ret, ptr::null_mut());
            let ret = Box::from_raw(ret);
            deserialized = ret.into_native();
        }
        assert_eq!(deserialized, tx);
    }

    #[test]
    fn test_deserialize_transaction_bad() {
        let serialized_bad = vec![0u8; 3];
        let deserialized;
        unsafe {
            deserialized = deserialize_transaction(serialized_bad.as_ptr(), serialized_bad.len());
        }
        assert_eq!(deserialized, ptr::null_mut());
    }

    #[test]
    fn test_get_pubkey_string() {
        let pubkey = Pubkey::new_rand();
        let str_native = format!("{}", pubkey);
        let str_c;
        unsafe {
            str_c = CString::from_raw(get_pubkey_string(&pubkey));
        }
        let str_c = str_c.into_string().unwrap();
        assert_eq!(str_native, str_c);
    }

    #[test]
    fn test_transaction_partial_sign() {
        let key_native = KeypairNative::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let mut tx_native = system_transaction::transfer(&key_native, &to, 50, blockhash);
        let tx = Box::into_raw(Box::new(Transaction::from_native(tx_native.clone())));
        let key = Keypair::from_native(&key_native);
        let tx2;
        unsafe {
            assert_eq!(0, transaction_partial_sign(tx, &key, 1, &blockhash));
            tx2 = Box::from_raw(tx).into_native();
        }
        tx_native.partial_sign(&[&key_native], blockhash);
        assert_eq!(tx_native, tx2);
    }
}
