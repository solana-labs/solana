//! TemplateSigner is a protocol to coordinate signing of a message that fits a specific template.
//! Terminology
//!     * `template` - The partially filled message that is to be signed by the signer.  The
//!       template contains the Key for the kind of cryptographic key the message is signed with,
//!       a mask that identifies the undefined regions of the message and signature to proove that
//!       the signer has control of the Key.
//!
//!     * `signer` - The Pubkey of the entity that sign the template.  This is a Solana Pubkey and
//!       this key is different and unreleated to the template.
//!
//!     * `escrow` - The tokens held by the contract
//!
//!     * `contract` - The instance of the signing obligation.  
//!
//!     * `guarantee` - The tokens held as a guarantee that the `signer` will actually sign the
//!       message when the request is made.
//!
//!     * `owner` - The current owner of the contract.  Only the owner can make the request to
//!     sign.
//!
//! A template is composed of an arbitratry key, a message, and a mask as well as proof of owning
//! that key.
//!
//! A Contract to sign a message that fits the template is created with some `guarantee`.  The
//! `guarantee` is only used for ensure that signing is completed, it is not there to ensure
//! whatever is represented by the template message.
//!
//! When the Contract is bought the first time, the `Contract::owner Pubkey` is changed, an the
//! `Contract::escrow tokens` is filled with the tokens that the contract was bought for.  Once the
//! contract has been signed the escrow tokens are claimed by the `signer`.  If the `signer` fails
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

use bincode::{deserialize, serialize_into};
use solana_program_interface::account::KeyedAccount;
use solana_program_interface::pubkey::Pubkey;

enum Key {
    Noop(Vec<u8>),
}

struct Template {
    /// a key that will be used to sign a message that fits the template
    key: Key,
    /// message that will be signed
    message: Vec<u8>,
    /// mask for the message. 0 bits are to be filled out by the request.
    mask: Vec<u8>,
    /// signature of the mask
    sig: Vec<u8>,
}

struct Request {
    /// (request.message & template.mask) == (template.message & template.mask)
    message: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Contract {
    /// Template that will be filled by the signer upon request
    template: Template,
    /// The identity that can claim the escrow and `guarantee` if the template is signed.
    signer: Pubkey,
    /// The signing `guarantee`.
    guarantee: u64,
    /// The tokens in escrow to be released to the signer if they sign the template.
    escrow: u64,
    /// Request to sign must be made before template_timeout.
    template_timeout: u64,
    /// Once request to sign has been made the signer needs to submit the signature before this
    /// timeout.
    requested_timeout: u64,
    /// Once the template has been signed and `signed_timeout` expires the `guarantee` and
    /// escrow is claimable by the signer.
    signed_timeout: u64,
    /// Current owner identity. This is the identity that can request a message to be signed.
    owner: Pubkey,
    /// current state
    state: State,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum State {
    Created,
    EscrowFilled,
    Requested {
        pending: Request,
    },
    Signed {
        pending: Request,
        sig: Vec<u8>,
    },
    ConcurrentSignature {
        pending: Option<Request>,
        concurrent: Request,
        sig: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Message {
    /// Message to initalize the template.
    /// contract.state must be `State::Created`
    /// contract.escrow must be 0
    /// * account[0] - must be signed, current owner
    /// * account[1] - the contract
    Init(Contract),
    /// * u64 - Tokens transfered to the contract.
    /// * account[0] - must be signed, current owner
    /// * account[1] - the contract
    /// * account[2] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::InitEscrow, deposit the `escrow_token` amount into the contract
    InitEscrow(u64),

    /// * account[0] - must be signed, current owner
    /// * account[1] - the contract
    /// * account[2] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::ChangeOwner
    ChangeOwner,

    /// * account[0] - must be signed, current owner
    /// * account[1] - the contract
    ///
    /// Message respond with a signed tx.  If the tx is valid, the state is moved to Signed
    Request(Request),

    /// * account[0] - any caller
    /// * account[1] - the contract
    ///
    /// Valid signature that is a response
    SignatureResponse(Vec<u8>),

    /// * account[0] - any caller
    /// * account[1] - the contract
    ///
    /// Signature prooves that there was a double spend
    ConcurrentSignature { msg: Vec<u8>, sig: Vec<u8> },

    /// Message to claim the tokens
    /// * (State::Created | State::EscrowFilled) & `template_timeout`
    ///    The `signer` can claim `guarantee`, the `owner` can claim the `escrow`.    
    /// * Requested & `requested_timeout`
    ///    The the `owner` can claim the `escrow` and the `guarantee`.    
    /// * Signed & `signed_timeout`
    ///    The the `signer` can claim the `escrow` and the `guarantee`.    
    /// * ConcurrentSignature
    ///    The the `owner` can claim the `escrow` and the `guarantee`.    
    Claim,
}

impl Contract {
    fn verify_init(&self) -> bool {
        self.escrow == 0 && self.state == State::Created
    }
    fn template_timeout(&self) -> bool {
        //TODO: transaction height is necessary for this check
        false
    }
    fn requested_timeout(&self) -> bool {
        //TODO: transaction height is necessary for this check
        false
    }
    pub fn init_escrow(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        escrow: u64,
    ) -> Result<(), ()> {
        if contract.state != State::Created {
            return Err(());
        }
        if contract.template_timeout() {
            return Err(());
        }
        //TODO: keyed_accounts[0].is_signed()?;
        contract.owner = keyed_accounts[2].owner;
        keyed_accounts[1].tokens += escrow;
        keyed_accounts[0].tokens -= escrow;
        contract.escrow = escrow;
        contract.state = State::EscrowFilled;
        Ok(())
    }
    pub fn change_owner(&mut self, keyed_accounts: &mut Vec<KeyedAccount>) -> Result<(), ()> {
        if contract.state != State::EscrowFilled {
            return Err(());
        }
        if contract.template_timeout() {
            return Err(());
        }
        //TODO: keyed_accounts[0].is_signed()?;
        if contract.owner != keyed_accounts[0].pubkey {
            return Err(());
        }
        contract.owner = keyed_accounts[2].owner;
        Ok(())
    }
    pub fn start_request(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        request: Request,
    ) -> Result<(), ()> {
        let mut contract = deserialize(&keyed_accounts[1].userdata);
        if contract.state != State::EscrowFilled {
            return Err(());
        }
        if contract.template_timeout() {
            return Err(());
        }
        if contract.owner != keyed_accounts[0].pubkey {
            return Err(());
        }
        if !contract.template.verify_request(request) {
            return Err(());
        }
        //TODO: keyed_accounts[0].is_signed()?;
        contract.state = Requested(request);
        Ok(())
    }
    pub fn signature_response(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        sig: Vec<u8>,
    ) -> Result<(), ()> {
        match contract.state {
            State::Requested { pending } => {
                if contract.request_timeout() {
                    return Err(());
                }
                contract.state = State::Signed {
                    pending: request,
                    sig,
                };
            }
            _ => return Err(()),
        }
    }
    pub fn concurrent_signature(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        msg: Vec<u8>,
        sig: Vec<u8>,
    ) -> Result<(), ()> {
        match contract.state {
            State::Requested { pending } => {
                if msg != pending || !contract.template.check_sig(msg, sig) {
                    return Err(());
                }
                if contract.requested_timeout() {
                    return Err(());
                }
                contract.state = State::ConcurrentSignature {
                    pending: Some(pending),
                    concurrent: msg,
                    sig,
                };
            }
            State::Signed { pending, sig } => {
                if msg != pending || !contract.template.check_sig(msg, sig) {
                    return Err(());
                }
                if contract.signed_timeout() {
                    return Err(());
                }
                contract.state = State::ConcurrentSignature {
                    pending: Some(pending),
                    concurrent: msg,
                    sig,
                };
            }
            _ => {
                if contract.template_timeout() {
                    return Err(());
                }
                if !contract.template.check_sig(msg, sig) {
                    return Err(());
                }
                contract.state = State::ConcurrentSignature {
                    pending: None,
                    concurrent: msg,
                    sig,
                };
            }
        }
    }
}

fn process_data(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> Result<(), ()> {
    let message = deserialize(data);
    match message {
        Ok(Message::Init(contract)) => {
            if !contract.verify_init() {
                return Err(());
            }
            //TODO: keyed_accounts[0].is_signed()?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::InitEscrow(escrow)) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            contract.init_escrow(keyed_accounts, escrow)?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::ChangeOwner) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            contract.change_owner()?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::Request(request)) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            contract.start_request()?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::SignatureResponse(sig)) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            contract.signature_response()?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::ConcurrentSignature { msg, sig }) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            contract.concurrent_signature()?;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        _ => Err(()),
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
    process_data(keyed_accounts, data).is_ok()
}
