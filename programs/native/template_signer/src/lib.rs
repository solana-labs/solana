//! TemplateSigner is a protocol to coordinate signing of a message that fits a specific template.
//!
//! Template is composed of an arbitratry key, a message, and a mask as well as proof of owning
//! that key.
//!
//! A Contract for the template is created with some `guarantee`.  The `guarantee` is only used for
//! ensure that signing is completed, it is not there to ensure whatever is represented by the
//! template message.
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
    key: Key,
    /// message that will be signed
    message: Vec<u8>,
    /// mask for the message. 0 bits are to be filled out by the request.
    mask: Vec<u8>,
    /// signature of the mask
    sig: Vec<u8>,
}

struct Request {
    // (request.message & template.mask) == (template.message & template.mask)
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

    /// Valid signature that is a response
    SignatureResponse(Vec<u8>),

    /// Valid signature that is a response
    ConcurrentSignature { msg: Vec<u8>, sig: Vec<u8> },

    /// Message to claim the tokens
    /// * (State::Created | State::EscrowFilled) & `template_timeout`
    ///    The signer can claim guarantee, the owner can claim the escrow.    
    /// * Requested & `requested_timeout`
    ///    The the owner can claim the escrow and the guarantee.    
    /// * Signed & `signed_timeout`
    ///    The the signer can claim the escrow and the guarantee.    
    /// * ConcurrentSignature
    ///    The the owner can claim the escrow and the guarantee.    
    Claim,
}

impl Contract {
    fn verify_init(&self) -> bool {
        self.escrow == 0 && self.state == State::Created
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
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::ChangeOwner) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            if contract.state != State::EscrowFilled {
                return Err(());
            }
            if contract.template_timeout() {
                return Err(());
            }
            //TODO: keyed_accounts[0].is_signed()?;
            contract.owner = keyed_accounts[2].owner;
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::Request(request)) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            if contract.state != State::EscrowFilled {
                return Err(());
            }
            if contract.template_timeout() {
                return Err(());
            }
            //TODO: keyed_accounts[0].is_signed()?;
            contract.state = Requested(request);
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::SignatureResponse(sig)) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            if contract.request_timeout() {
                return Err(());
            }
            if let State::Requested(req) = contract.state {
                if !contract.template.validate(req, sig) {
                    return Err(());
                }
            } else {
                return Err(());
            }
            //TODO: keyed_accounts[0].is_signed()?;
            contract.state = State::Signed {
                pending: request,
                sig,
            };
            serialize_into(&keyed_accounts[1].userdata, &contract)?;
        }
        Ok(Message::ConcurrentSignature { msg, sig }) => {
            let mut contract = deserialize(&keyed_accounts[1].userdata);
            match contract.state {
                State::Requested { pending }
                    if msg != pending && contract.template.check_sig(msg, sig) =>
                {
                    if contract.requested_timeout() {
                        return Err(());
                    }
                    contract.state = State::ConcurrentSignature {
                        pending: Some(pending),
                        concurrent: msg,
                        sig,
                    };
                }
                State::Signed { pending, sig }
                    if msg != pending && contract.template.check_sig(msg, sig) =>
                {
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
