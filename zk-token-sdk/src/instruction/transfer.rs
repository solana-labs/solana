use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            discrete_log::*,
            elgamal::{
                DecryptHandle, ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey,
            },
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        instruction::{Role, Verifiable},
        range_proof::RangeProof,
        sigma_proofs::{equality_proof::EqualityProof, validity_proof::AggregatedValidityProof},
        transcript::TranscriptProtocol,
    },
    arrayref::{array_ref, array_refs},
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
    std::convert::TryInto,
};

/// Constant for 2^32
const TWO_32: u64 = 4294967296;

#[derive(Clone)]
#[repr(C)]
pub(crate) struct ElGamalGroupEncryption {
    pub commitment: PedersenCommitment,
    pub source: DecryptHandle,
    pub dest: DecryptHandle,
    pub auditor: DecryptHandle,
}

impl ElGamalGroupEncryption {
    pub fn to_bytes(&self) -> [u8; 128] {
        let mut bytes = [0u8; 128];
        bytes[..32].copy_from_slice(&self.commitment.to_bytes());
        bytes[32..64].copy_from_slice(&self.source.to_bytes());
        bytes[64..96].copy_from_slice(&self.dest.to_bytes());
        bytes[96..128].copy_from_slice(&self.auditor.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 128];
        let (commitment, source, dest, auditor) = array_refs![bytes, 32, 32, 32, 32];

        let commitment = PedersenCommitment::from_bytes(commitment).ok_or(ProofError::Verification)?;
        let source = DecryptHandle::from_bytes(source).ok_or(ProofError::Verification)?;
        let dest = DecryptHandle::from_bytes(dest).ok_or(ProofError::Verification)?;
        let auditor = DecryptHandle::from_bytes(auditor).ok_or(ProofError::Verification)?;

        Ok(Self {
            commitment,
            source,
            dest,
            auditor,
        })
    }
}

impl ElGamalGroupEncryption {
    fn new(
        amount: u32,
        transfer_pubkeys: &TransferPubkeys,
    ) -> (ElGamalGroupEncryption, PedersenOpening) {
        let (commitment, opening) = Pedersen::new(amount);
        let group_encryption = ElGamalGroupEncryption {
            commitment,
            source: transfer_pubkeys.source.decrypt_handle(&opening),
            dest: transfer_pubkeys.dest.decrypt_handle(&opening),
            auditor: transfer_pubkeys.auditor.decrypt_handle(&opening),
        };

        (group_encryption, opening)
    }
}

/// Split u64 number into two u32 numbers
#[cfg(not(target_arch = "bpf"))]
pub fn split_u64_into_u32(amount: u64) -> (u32, u32) {
    let lo = amount as u32;
    let hi = (amount >> 32) as u32;

    (lo, hi)
}

fn combine_u32_ciphertexts(
    ciphertext_lo: &ElGamalCiphertext,
    ciphertext_hi: &ElGamalCiphertext,
) -> ElGamalCiphertext {
    ciphertext_lo + &(ciphertext_hi * &Scalar::from(TWO_32))
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferData {
    /// Group encryption of the low 32 bits of the transfer amount
    pub ciphertext_lo: pod::ElGamalGroupEncryption,

    /// Group encryption of the high 32 bits of the transfer amount
    pub ciphertext_hi: pod::ElGamalGroupEncryption,

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_pubkeys: pod::TransferPubkeys,

    /// The final spendable ciphertext after the transfer
    pub new_spendable_ct: pod::ElGamalCiphertext,

    /// Zero-knowledge proofs for Transfer
    pub proof: TransferProof,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        // amount of the transfer
        transfer_amount: u64,

        // available balance in the source account as u64
        spendable_balance: u64,

        // available balance in the source account as ElGamalCiphertext
        spendable_balance_ciphertext: &ElGamalCiphertext,

        // source account ElGamal keypair
        keypair_source: &ElGamalKeypair,

        // destination account ElGamal pubkey
        pubkey_dest: &ElGamalPubkey,

        // auditor ElGamal pubkey
        pubkey_auditor: &ElGamalPubkey,
    ) -> Self {
        // split and encrypt transfer amount
        let (amount_lo, amount_hi) = split_u64_into_u32(transfer_amount);

        // encrypt transfer amount
        let transfer_pubkeys = TransferPubkeys {
            source: &keypair_source.public,
            dest: pubkey_dest,
            auditor: pubkey_auditor,
        };

        let (ciphertext_lo, opening_lo) = ElGamalGroupEncryption::new(amount_lo, &transfer_pubkeys);
        let (ciphertext_hi, opening_hi) = ElGamalGroupEncryption::new(amount_hi, &transfer_pubkeys);

        // subtract transfer amount from the spendable ciphertext
        let new_spendable_balance = spendable_balance - transfer_amount;

        let transfer_amount_lo_source = ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: ciphertext_lo.source,
        };

        let transfer_amount_hi_source = ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: ciphertext_hi.source,
        };

        let new_spendable_ciphertext = spendable_balance_ciphertext
            - combine_u32_ciphertexts(&transfer_amount_lo_source, &transfer_amount_hi_source);

        // generate transfer proof
        let mut transcript = TransferProof::transcript_new();
        let pod_transfer_pubkeys = TransferProof::transcript_append_pubkeys(
            b"transfer-pubkeys",
            &transfer_pubkeys,
            &mut transcript,
        );
        let pod_transfer_ciphertext_lo = TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext-lo",
            &ciphertext_lo,
            &mut transcript,
        );
        let pod_transfer_ciphertext_hi = TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext-hi",
            &ciphertext_hi,
            &mut transcript,
        );

        let proof = TransferProof::new(
            (amount_lo, amount_hi),
            keypair_source,
            (&pubkey_dest, &pubkey_auditor),
            (&ciphertext_lo, &opening_lo),
            (&ciphertext_hi, &opening_hi),
            (new_spendable_balance, &new_spendable_ciphertext),
            &mut transcript,
        );

        Self {
            ciphertext_lo: pod_transfer_ciphertext_lo,
            ciphertext_hi: pod_transfer_ciphertext_hi,
            transfer_pubkeys: pod_transfer_pubkeys,
            new_spendable_ct: new_spendable_ciphertext.into(),
            proof,
        }
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_lo: ElGamalGroupEncryption = self.ciphertext_lo.try_into()?;

        let handle_lo = match role {
            Role::Source => ciphertext_lo.source,
            Role::Dest => ciphertext_lo.dest,
            Role::Auditor => ciphertext_lo.auditor,
        };

        Ok(ElGamalCiphertext {
            commitment: ciphertext_lo.commitment,
            handle: handle_lo,
        })
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let ciphertext_hi: ElGamalGroupEncryption = self.ciphertext_hi.try_into()?;

        let handle_hi = match role {
            Role::Source => ciphertext_hi.source,
            Role::Dest => ciphertext_hi.dest,
            Role::Auditor => ciphertext_hi.auditor,
        };

        Ok(ElGamalCiphertext {
            commitment: ciphertext_hi.commitment,
            handle: handle_hi,
        })
    }

    /// Decrypts transfer amount from transfer data
    ///
    /// TODO: This function should run in constant time. Use `subtle::Choice` for the if statement
    /// and make sure that the function does not terminate prematurely due to errors
    ///
    /// TODO: Define specific error type for decryption error
    pub fn decrypt_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.ciphertext_lo(role)?;
        let ciphertext_hi = self.ciphertext_hi(role)?;

        let amount_lo = ciphertext_lo.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);
        let amount_hi = ciphertext_hi.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);

        if let (Some(amount_lo), Some(amount_hi)) = (amount_lo, amount_hi) {
            Ok((amount_lo as u64) + (TWO_32 * amount_hi as u64))
        } else {
            Err(ProofError::Verification)
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for TransferData {
    fn verify(&self) -> Result<(), ProofError> {
        let transfer_commitments = TransferCommitments {
            lo: self.encrypted_transfer_amount.amount_comm_lo,
            hi: self.encrypted_transfer_amount.amount_comm_hi,
        };

        self.proof.verify(
            &transfer_commitments,
            &self.encrypted_transfer_amount.decrypt_handles_lo,
            &self.encrypted_transfer_amount.decrypt_handles_hi,
            &self.new_spendable_ct,
            &self.transfer_public_keys,
        )
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferProof {
    /// New Pedersen commitment for the remaining balance in source
    pub source_commitment: pod::PedersenCommitment,

    /// Associated equality proof
    pub equality_proof: pod::EqualityProof,

    /// Associated ciphertext validity proof
    pub validity_proof: pod::AggregatedValidityProof,

    // Associated range proof
    pub range_proof: pod::RangeProof128,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferProof {
    fn transcript_new() -> Transcript {
        let mut transcript = Transcript::new(b"transfer-proof");
        transcript
    }

    fn transcript_append_pubkeys(
        label: &'static [u8],
        transfer_pubkeys: &TransferPubkeys,
        transcript: &mut Transcript,
    ) -> pod::TransferPubkeys {
        let transfer_pubkeys_bytes = transfer_pubkeys.to_bytes();

        transcript.append_message(b"dom-sep", label);
        transcript.append_message(b"transfer-pubkeys", &transfer_pubkeys_bytes);

        pod::TransferPubkeys(transfer_pubkeys_bytes)
    }

    fn transcript_append_ciphertext(
        label: &'static [u8],
        ciphertext: &ElGamalGroupEncryption,
        transcript: &mut Transcript,
    ) -> pod::ElGamalGroupEncryption {
        let ciphertext_bytes = ciphertext.to_bytes();

        transcript.append_message(b"dom-sep", label);
        transcript.append_message(b"amount-ciphertext-lo", &ciphertext_bytes);

        pod::ElGamalGroupEncryption(ciphertext_bytes)
    }

    pub fn new(
        (transfer_amount_lo, transfer_amount_hi): (u32, u32),
        keypair_source: &ElGamalKeypair,
        (pubkey_dest, pubkey_auditor): (&ElGamalPubkey, &ElGamalPubkey),
        (ciphertext_lo, opening_lo): (&ElGamalGroupEncryption, &PedersenOpening),
        (ciphertext_hi, opening_hi): (&ElGamalGroupEncryption, &PedersenOpening),
        (source_new_balance, source_new_balance_ciphertext): (u64, &ElGamalCiphertext),
        transcript: &mut Transcript,
    ) -> Self {
        // generate a Pedersen commitment for the remaining balance in source
        let (source_commitment, source_opening) = Pedersen::new(source_new_balance);

        // generate equality_proof
        let equality_proof = EqualityProof::new(
            keypair_source,
            source_new_balance_ciphertext,
            source_new_balance,
            &source_opening,
            &mut transcript,
        );

        // generate ciphertext validity proof
        let validity_proof = AggregatedValidityProof::new(
            (pubkey_dest, pubkey_auditor),
            (transfer_amount_lo, transfer_amount_hi),
            (opening_lo, opening_hi),
            &mut transcript,
        );

        // generate the range proof
        let range_proof = RangeProof::new(
            vec![
                source_new_balance,
                transfer_amount_lo as u64,
                transfer_amount_hi as u64,
            ],
            vec![64, 32, 32],
            vec![&source_opening, opening_lo, opening_hi],
            &mut transcript,
        );

        Self {
            source_commitment: source_commitment.into(),
            equality_proof: equality_proof.try_into().expect("equality proof"),
            validity_proof: validity_proof.try_into().expect("validity proof"),
            range_proof: range_proof.try_into().expect("range proof"),
        }
    }

    pub fn verify(
        self,
        amount_comms: &TransferCommitments,
        decryption_handles_lo: &TransferDecryptHandles,
        decryption_handles_hi: &TransferDecryptHandles,
        new_spendable_ct: &pod::ElGamalCiphertext,
        transfer_public_keys: &TransferPubkeys,
    ) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        let commitment: PedersenCommitment = self.source_commitment.try_into()?;
        let equality_proof: EqualityProof = self.equality_proof.try_into()?;
        let aggregated_validity_proof: AggregatedValidityProof = self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // add a domain separator to record the start of the protocol
        transcript.transfer_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let source_pk: ElGamalPubkey = transfer_public_keys.source_pk.try_into()?;
        let new_spendable_ct: ElGamalCiphertext = (*new_spendable_ct).try_into()?;

        let P_EG = source_pk.get_point();
        let C_EG = new_spendable_ct.commitment.get_point();
        let D_EG = new_spendable_ct.handle.get_point();
        let C_Ped = commitment.get_point();

        // append all current state to the transcript
        transcript.append_point(b"P_EG", &P_EG.compress());
        transcript.append_point(b"C_EG", &C_EG.compress());
        transcript.append_point(b"D_EG", &D_EG.compress());
        transcript.append_point(b"C_Ped", &C_Ped.compress());

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(&source_pk, &new_spendable_ct, &commitment, &mut transcript)?;

        // TODO: record destination and auditor public keys to transcript
        let dest_elgamal_pubkey: ElGamalPubkey = transfer_public_keys.dest_pk.try_into()?;
        let auditor_elgamal_pubkey: ElGamalPubkey = transfer_public_keys.auditor_pk.try_into()?;

        let amount_comm_lo: PedersenCommitment = amount_comms.lo.try_into()?;
        let amount_comm_hi: PedersenCommitment = amount_comms.hi.try_into()?;

        let handle_lo_dest: DecryptHandle = decryption_handles_lo.dest.try_into()?;
        let handle_hi_dest: DecryptHandle = decryption_handles_hi.dest.try_into()?;

        let handle_lo_auditor: DecryptHandle = decryption_handles_lo.auditor.try_into()?;
        let handle_hi_auditor: DecryptHandle = decryption_handles_hi.auditor.try_into()?;

        // TODO: validity proof
        aggregated_validity_proof.verify(
            (&dest_elgamal_pubkey, &auditor_elgamal_pubkey),
            (&amount_comm_lo, &amount_comm_hi),
            (&handle_lo_dest, &handle_hi_dest),
            (&handle_lo_auditor, &handle_hi_auditor),
            &mut transcript,
        )?;

        // verify range proof
        range_proof.verify(
            vec![
                &self.source_commitment.into(),
                &amount_comms.lo.into(),
                &amount_comms.hi.into(),
            ],
            vec![64_usize, 32_usize, 32_usize],
            &mut transcript,
        )?;

        Ok(())
    }
}

/// The ElGamal public keys needed for a transfer
#[derive(Clone)]
#[repr(C)]
pub(crate) struct TransferPubkeys<'pk> {
    pub source: &'pk ElGamalPubkey,
    pub dest: &'pk ElGamalPubkey,
    pub auditor: &'pk ElGamalPubkey,
}

impl<'pk> TransferPubkeys<'pk> {
    fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&self.source.to_bytes());
        bytes[32..64].copy_from_slice(&self.dest.to_bytes());
        bytes[64..96].copy_from_slice(&self.auditor.to_bytes());
        bytes
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct EncryptedTransferAmount {
    pub amount_comm_lo: pod::PedersenCommitment,

    pub amount_comm_hi: pod::PedersenCommitment,

    /// The decryption handles that allow decryption of the lo-bits of the transfer amount
    pub decrypt_handles_lo: TransferDecryptHandles,

    /// The decryption handles that allow decryption of the hi-bits of the transfer amount
    pub decrypt_handles_hi: TransferDecryptHandles,
}

/// The decryption handles needed for a transfer
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferDecryptHandles {
    pub source: pod::DecryptHandle,  // 32 bytes
    pub dest: pod::DecryptHandle,    // 32 bytes
    pub auditor: pod::DecryptHandle, // 32 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferCommitments {
    pub lo: pod::PedersenCommitment,
    pub hi: pod::PedersenCommitment,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct EncryptedTransferFee {
    /// The transfer fee commitment
    pub fee_comm: pod::PedersenCommitment,
    /// The decryption handle for destination ElGamal pubkey
    pub decrypt_handle_dest: pod::DecryptHandle,
    /// The decryption handle for fee collector ElGamal pubkey
    pub decrypt_handle_fee_collector: pod::DecryptHandle,
}

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_comms(
    comm_lo: PedersenCommitment,
    comm_hi: PedersenCommitment,
) -> PedersenCommitment {
    comm_lo + comm_hi * Scalar::from(TWO_32)
}

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_handles(handle_lo: DecryptHandle, handle_hi: DecryptHandle) -> DecryptHandle {
    handle_lo + handle_hi * Scalar::from(TWO_32)
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_transfer_correctness() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();
        let dest_pk = ElGamalKeypair::new_rand().public;
        let auditor_pk = ElGamalKeypair::new_rand().public;

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ct = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            spendable_balance,
            spendable_ct,
            &source_keypair,
            dest_pk,
            auditor_pk,
        );

        assert!(transfer_data.verify().is_ok());
    }

    #[test]
    fn test_source_dest_ciphertext() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::new_rand();

        let ElGamalKeypair {
            public: dest_pk,
            secret: dest_sk,
        } = ElGamalKeypair::new_rand();

        let ElGamalKeypair {
            public: auditor_pk,
            secret: auditor_sk,
        } = ElGamalKeypair::new_rand();

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ct = source_keypair.public.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            spendable_balance,
            spendable_ct,
            &source_keypair,
            dest_pk,
            auditor_pk,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Source, &source_keypair.secret)
                .unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data.decrypt_amount(Role::Dest, &dest_sk).unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data
                .decrypt_amount(Role::Auditor, &auditor_sk)
                .unwrap(),
            55_u64,
        );
    }
}
