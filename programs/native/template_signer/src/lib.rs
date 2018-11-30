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
//!     * `escrow` - The tokens held by the SigningContract.
//!
//!     * `SigningContract` - An instance of the statem machine for the specific template to be signed.
//!
//!     * `guarantee` - The tokens held as a guarantee that the `signer` will actually sign the
//!       message when the request is made.
//!
//!     * `owner` - The current owner of the SigningContract.  Only the owner can make the request to
//!     sign.
//!
//! A template is composed of an arbitratry key, a message, and a mask as well as proof of owning
//! that key.
//!
//! A SigningContract to sign a message that fits the template is created with some `guarantee`.  The
//! `guarantee` is only used for ensure that signing is completed, it is not there to ensure
//! whatever is represented by the template message.
//!
//! When the SigningContract is bought the first time, the `SigningContract::owner Pubkey` is changed, an the
//! `SigningContract::escrow tokens` is filled with the tokens that the SigningContract was bought for.  Once the
//! SigningContract has been signed the escrow tokens are claimed by the `signer`.  If the `signer` fails
//! to fulfill the request the escrow tokens can be claimed by the current owner.
//!
//! A new owner can sell this SigningContract again.  The second sale doesn't modify the escrow tokens.
//!
//! An owner can make a request for a signature.  This must be done before the
//! `SigningContract::template_timeout`.  Once the request has been made, the `signer` has
//! `requested_timeout` amount of time to fulfill the request.  If the `signer` fails to do so, the
//! `escrow` and `guarantee` can be claimed by the owner.
//!
//! Once the tempalte has been signed, there is another timeout before the `escrow`  and
//! `guarantee` are released to the `signer`.  If another proof of signing with the `Template::key` is
//! submitted, the `owner` can claim the `escrow` and `guarantee` tokens.
//!
//! If the template expires, the current `owner` can claim the `escrow`, and the `signer` can claim
//! the `guarantee`.
//!
//! Future Work:
//! * adding real keys, secp256k1 and ed25519 curves
//! * advanced template langauge, so a template could be aware of version numbers inside the
//!   message

extern crate bincode;
extern crate serde;
extern crate solana_sdk;
#[macro_use]
extern crate serde_derive;

use bincode::{deserialize, serialize_into};
use solana_sdk::account::KeyedAccount;
use solana_sdk::pubkey::Pubkey;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Key {
    Noop(Vec<u8>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Template {
    /// a key that will be used to sign a message that fits the template
    key: Key,
    /// message that will be signed
    msg: Vec<u8>,
    /// mask for the message. 0 bits are to be filled out by the request.
    mask: Vec<u8>,
    /// signature of the mask
    sig: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Request {
    /// (request.msg & template.mask) == (template.msg & template.mask)
    msg: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct SigningContract {
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
        concurrent: Vec<u8>,
        sig: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Message {
    /// Message to initalize the template.
    /// SigningContract.state must be `State::Created`
    /// SigningContract.escrow must be 0
    /// * account[0] - must be signed, current owner
    /// * account[1] - the SigningContract
    Init(SigningContract),
    /// * u64 - Tokens transfered to the SigningContract.
    /// * account[0] - must be signed, current owner
    /// * account[1] - the SigningContract
    /// * account[2] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::InitEscrow, deposit the `escrow_token` amount into the
    /// SigningContract
    InitEscrow(u64),

    /// * account[0] - must be signed, current owner
    /// * account[1] - the SigningContract
    /// * account[2] - the new owner
    ///
    /// This should be executed in an atomic transaction with
    /// Instruction 0, System::Move tokens from new owner to current owner
    /// Instruction 1, Signer::ChangeOwner
    ChangeOwner,

    /// * account[0] - must be signed, current owner
    /// * account[1] - the SigningContract
    ///
    /// Message respond with a signed tx.  If the tx is valid, the state is moved to Signed
    Request(Request),

    /// * account[0] - any caller
    /// * account[1] - the SigningContract
    ///
    /// Valid signature that is a response
    SignatureResponse(Vec<u8>),

    /// * account[0] - any caller
    /// * account[1] - the SigningContract
    ///
    /// Signature prooves that there was a double spend
    ConcurrentSignature { msg: Vec<u8>, sig: Vec<u8> },

    /// * account[0] - caller
    /// * account[1] - the SigningContract
    ///
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

impl Key {
    fn check_sig(&self, _msg: &[u8], _sig: &[u8]) -> bool {
        //TODO: implement this
        true
    }
}
impl Template {
    fn verify_request(&self, request: &Request) -> Result<(), Error> {
        for ((r, m), mask) in request.msg.iter().zip(&self.msg).zip(&self.mask) {
            if *r & *mask != *m & *mask {
                return Err(Error::Error);
            }
        }
        Ok(())
    }
}
impl SigningContract {
    fn template_timeout(&self) -> bool {
        //TODO: transaction height is necessary for this check
        false
    }
    fn requested_timeout(&self) -> bool {
        //TODO: transaction height is necessary for this check
        false
    }
    fn signed_timeout(&self) -> bool {
        //TODO: transaction height is necessary for this check
        false
    }
    fn verify_init(&self, keyed_accounts: &mut Vec<KeyedAccount>) -> Result<(), Error> {
        if !(self.escrow == 0 && self.state == State::Created) {
            return Err(Error::Error);
        }
        //TODO: keyed_accounts[0].is_signed()?;
        if self.owner != *keyed_accounts[0].key {
            return Err(Error::Error);
        }
        if !self
            .template
            .key
            .check_sig(&self.template.mask, &self.template.sig)
        {
            return Err(Error::Error);
        }
        Ok(())
    }
    pub fn init_escrow(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        escrow: u64,
    ) -> Result<(), Error> {
        if self.state != State::Created {
            return Err(Error::Error);
        }
        if self.template_timeout() {
            return Err(Error::Error);
        }
        //TODO: keyed_accounts[0].is_signed()?;
        self.owner = *keyed_accounts[2].key;
        keyed_accounts[1].account.tokens += escrow;
        keyed_accounts[0].account.tokens -= escrow;
        self.escrow = escrow;
        self.state = State::EscrowFilled;
        Ok(())
    }
    pub fn change_owner(&mut self, keyed_accounts: &mut Vec<KeyedAccount>) -> Result<(), Error> {
        if self.state != State::EscrowFilled {
            return Err(Error::Error);
        }
        if self.template_timeout() {
            return Err(Error::Error);
        }
        //TODO: keyed_accounts[0].is_signed()?;
        if self.owner != *keyed_accounts[0].key {
            return Err(Error::Error);
        }
        self.owner = *keyed_accounts[2].key;
        Ok(())
    }
    pub fn start_request(
        &mut self,
        keyed_accounts: &mut Vec<KeyedAccount>,
        request: Request,
    ) -> Result<(), Error> {
        if self.state != State::EscrowFilled {
            return Err(Error::Error);
        }
        if self.template_timeout() {
            return Err(Error::Error);
        }
        if self.owner != *keyed_accounts[0].key {
            return Err(Error::Error);
        }
        self.template.verify_request(&request)?;
        //TODO: keyed_accounts[0].is_signed()?;
        self.state = State::Requested { pending: request };
        Ok(())
    }
    pub fn signature_response(&mut self, sig: Vec<u8>) -> Result<(), Error> {
        let nxt = match self.state {
            State::Requested { ref pending } => {
                if self.requested_timeout() {
                    return Err(Error::Error);
                }
                if !self.template.key.check_sig(&pending.msg, &sig) {
                    return Err(Error::Error);
                }
                State::Signed {
                    pending: pending.clone(),
                    sig,
                }
            }
            _ => return Err(Error::Error),
        };
        self.state = nxt;
        Ok(())
    }
    pub fn concurrent_signature(&mut self, msg: Vec<u8>, sig: Vec<u8>) -> Result<(), Error> {
        let nxt = match self.state {
            State::Requested { ref pending } => {
                if msg != pending.msg || !self.template.key.check_sig(&msg, &sig) {
                    return Err(Error::Error);
                }
                if self.requested_timeout() {
                    return Err(Error::Error);
                }
                State::ConcurrentSignature {
                    pending: Some(pending.clone()),
                    concurrent: msg,
                    sig,
                }
            }
            State::Signed {
                ref pending,
                ref sig,
            } => {
                if msg != pending.msg || !self.template.key.check_sig(&msg, sig) {
                    return Err(Error::Error);
                }
                if self.signed_timeout() {
                    return Err(Error::Error);
                }
                State::ConcurrentSignature {
                    pending: Some(pending.clone()),
                    concurrent: msg,
                    sig: sig.clone(),
                }
            }
            _ => {
                if self.template_timeout() {
                    return Err(Error::Error);
                }
                if !self.template.key.check_sig(&msg, &sig) {
                    return Err(Error::Error);
                }
                State::ConcurrentSignature {
                    pending: None,
                    concurrent: msg,
                    sig,
                }
            }
        };
        self.state = nxt;
        Ok(())
    }
    pub fn claim(&mut self, _keyed_accounts: &mut Vec<KeyedAccount>) -> Result<(), Error> {
        //TODO: implement this
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Error,
}
impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for Error {
    fn from(_e: std::boxed::Box<bincode::ErrorKind>) -> Error {
        Error::Error
    }
}

fn process_data(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> Result<(), Error> {
    let message = deserialize(data);
    match message {
        Ok(Message::Init(contract)) => {
            contract.verify_init(keyed_accounts)?;
            //TODO: keyed_accounts[0].is_signed()?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::InitEscrow(escrow)) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.init_escrow(keyed_accounts, escrow)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::ChangeOwner) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.change_owner(keyed_accounts)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::Request(request)) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.start_request(keyed_accounts, request)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::SignatureResponse(sig)) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.signature_response(sig)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::ConcurrentSignature { msg, sig }) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.concurrent_signature(msg, sig)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Ok(Message::Claim) => {
            let mut contract: SigningContract = deserialize(&keyed_accounts[1].account.userdata)?;
            contract.claim(keyed_accounts)?;
            serialize_into(&mut keyed_accounts[1].account.userdata, &contract)?;
            Ok(())
        }
        Err(_) => Err(Error::Error),
    }
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
    process_data(keyed_accounts, data).is_ok()
}
