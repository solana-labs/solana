//! TemplateSigner is a protocol to coordinate signing of a message that fits a specific template.
//!
//! Template is composed of an arbitratry key, a message, and a mask as well as proof of owning
//! that key.
//!
//! A Contract for the template is created with some guarantee.  The guarantee is only used for
//! ensure that signing is completed, it is not there to ensure whatever is represented by the
//! template message.
//!
//! When the Contract is bought the first time, the `Contract::owner Pubkey` is changed, an the
//! `Contract::escrow tokens` is filled with the tokens that the contract was bought for.  Once the
//! contract has been fulfilled the escrow tokens are claimed by the `signer`.  If the `signer` fails
//! to fulfill the request the escrow tokens can be claimed by the current owner.
//!
//! A new owner can sell this contract again.  The second sale doesn't modify the escrow tokens.
//!
//! An owner can make a request for a signature.  This must be done before the
//! `Contract::template_timeout`.  Once the request has been made, the `signer` has
//! `requested_timeout` amount of time to fulfill the request.  If the `signer` fails to do so, the
//! `escrow` and `guarantee` can be claimed by the owner.
//!
//! Once the tempalte has been signed, there is another timeout before the `escrow`  and
//! `guarantee` are released to the `signer`.  If another proof of signing with the `Template::key` is
//! submitted, the `owner` can claim the `escrow` and `guarantee` tokens.
//!
//! If the template expires, the current `owner` can claim the `escrow`, and the `signer` can claim
//! the `guarantee`.

extern crate bincode;
extern crate serde;
extern crate solana_program_interface;
#[macro_use]
extern crate serde_derive;

use bincode::deserialize;
use solana_program_interface::account::KeyedAccount;
use solana_program_interface::pubkey::Pubkey;

enum Key {
    Secp256k1(Vec<u8>),
}

struct Template {
    key: Key,
    /// message that will be signed
    message: Vec<u8>,
    /// mask for the message. 0 bits are to be filled out by the request.
    mask: Vec<u8>,
    /// signature of (message & mask)
    sig: Vec<u8>,
}

struct Request {
    // (request.message & template.mask) == (template.message & template.mask)
    message: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Contract {
    // contains the pubkey of the template and the fulfilled challenge and the tempalte
    template: Template,
    // the identity that can claim the escrow and guarantee if the template is signed
    signer: Pubkey,
    // the settlement guarantee
    guarantee: u64,
    // the tokens in escrow to be released to the signer if they sign the template
    escrow: u64,
    // template must be requested before this timeout
    template_timeout: u64,
    // Once the template has been Requested, if this expires, the guarantee is claimable by the owner
    requested_timeout: u64,
    // Once the template has been Fulfilled and this expires the guarantee and escrow is claimable by the singer
    signed_timeout: u64,
    // Current owner identity. This is the identity that can request a message to be signed.
    owner: Pubkey,
    // current state
    state: State,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum State {
    Created,
    EscrowFilled,
    Requested(Request),
    Fulfilled(Request, Vec<u8>),
    Expired,
    BrokenBadSignature,
    BrokenTimeout,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Message {
    /// Message to initalize the template.
    /// contract.state must be `State::Created`
    /// contract.escrow must be 0
    Init { contract: Contract },
    /// * escrow - Tokens transfered to the contract.
    /// * account[0] - must be signed, current owner
    /// * account[1] - the contract
    /// * account[2] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::InitEscrow, deposit the `escrow_token` amount into the contract
    InitEscrow { escrow: u64 },

    /// * account[0] - must be signed, current owner
    /// * account[1] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::ChangeOwner
    ChangeOwner,

    /// Message respond with a signed tx.  If the tx is valid, the state is moved to Signed
    SignedTransaction { tx: Vec<u8> },
    /// Message to move the state to a Timeout state.
    Timeout,
}

impl Contract {
    fn verify_init(&self) -> bool {
        self.escrow == 0 && self.state == State::Created
    }
}

fn process_data(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> Result<(), ()> {
    let message = deserialize(data);
    match message {
        Ok(Message::Init { contract }) => {
            contract.verify_init()?;
        }
        _ => Err(()),
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
    process_data(keyed_accounts, data).is_ok()
}
