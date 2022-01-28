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
    pub new_spendable_ciphertext: pod::ElGamalCiphertext,

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
            source: keypair_source.public,
            dest: *pubkey_dest,
            auditor: *pubkey_auditor,
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

        // generate transcript and append all public inputs
        let mut transcript = TransferProof::transcript_new();

        let pod_transfer_pubkeys = pod::TransferPubkeys(transfer_pubkeys.to_bytes());
        TransferProof::transcript_append_pubkeys(
            b"transfer-pubkeys",
            &pod_transfer_pubkeys,
            &mut transcript,
        );

        let pod_ciphertext_lo: pod::ElGamalGroupEncryption = ciphertext_lo.into();
        TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext-lo",
            &pod_ciphertext_lo,
            &mut transcript,
        );

        let pod_ciphertext_hi: pod::ElGamalGroupEncryption = ciphertext_hi.into();
        TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext-hi",
            &pod_ciphertext_hi,
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
            ciphertext_lo: pod_ciphertext_lo,
            ciphertext_hi: pod_ciphertext_hi,
            transfer_pubkeys: pod_transfer_pubkeys,
            new_spendable_ciphertext: new_spendable_ciphertext.into(),
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
        // generate transcript and append all public inputs
        let mut transcript = TransferProof::transcript_new();

        TransferProof::transcript_append_pubkeys(
            b"transfer-pubkeys",
            &self.transfer_pubkeys,
            &mut transcript,
        );

        TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext_lo",
            &self.ciphertext_lo,
            &mut transcript,
        );

        TransferProof::transcript_append_ciphertext(
            b"transfer-ciphertext_hi",
            &self.ciphertext_hi,
            &mut transcript,
        );

        let ciphertext_lo = self.ciphertext_lo.try_into()?;
        let ciphertext_hi = self.ciphertext_hi.try_into()?;
        let transfer_pubkeys = self.transfer_pubkeys.try_into()?;
        let new_spendable_ciphertext = self.new_spendable_ciphertext.try_into()?;

        self.proof.verify(
            &ciphertext_lo,
            &ciphertext_hi,
            &transfer_pubkeys,
            &new_spendable_ciphertext,
            &mut transcript,
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
        transfer_pubkeys: &pod::TransferPubkeys,
        transcript: &mut Transcript,
    ) {
        transcript.append_message(b"dom-sep", label);
        transcript.append_message(b"transfer-pubkeys", &transfer_pubkeys.0);
    }

    fn transcript_append_ciphertext(
        label: &'static [u8],
        ciphertext: &pod::ElGamalGroupEncryption,
        transcript: &mut Transcript,
    ) {
        transcript.append_message(b"dom-sep", label);
        transcript.append_message(b"ciphertext-lo", &ciphertext.0);
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
        ciphertext_lo: &ElGamalGroupEncryption,
        ciphertext_hi: &ElGamalGroupEncryption,
        transfer_pubkeys: &TransferPubkeys,
        new_spendable_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ProofError> {
        let commitment: PedersenCommitment = self.source_commitment.try_into()?;
        let equality_proof: EqualityProof = self.equality_proof.try_into()?;
        let aggregated_validity_proof: AggregatedValidityProof = self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // extract the relevant scalar and Ristretto points from the inputs

        // let source_pk: ElGamalPubkey = transfer_public_keys.source_pk.try_into()?;
        // let new_spendable_ct: ElGamalCiphertext = (*new_spendable_ct).try_into()?;

        // let P_EG = source_pk.get_point();
        // let C_EG = new_spendable_ct.commitment.get_point();
        // let D_EG = new_spendable_ct.handle.get_point();
        // let C_Ped = commitment.get_point();

        // // append all current state to the transcript
        // transcript.append_point(b"P_EG", &P_EG.compress());
        // transcript.append_point(b"C_EG", &C_EG.compress());
        // transcript.append_point(b"D_EG", &D_EG.compress());
        // transcript.append_point(b"C_Ped", &C_Ped.compress());

        // verify equality proof
        //
        // TODO: we can also consider verifying equality and range proof in a batch
        equality_proof.verify(
            &transfer_pubkeys.source,
            &new_spendable_ciphertext,
            &commitment,
            &mut transcript,
        )?;

        // verify validity proof
        aggregated_validity_proof.verify(
            (&transfer_pubkeys.dest, &transfer_pubkeys.auditor),
            (&ciphertext_lo.commitment, &ciphertext_hi.commitment),
            (&ciphertext_lo.dest, &ciphertext_hi.dest),
            (&ciphertext_lo.auditor, &ciphertext_hi.auditor),
            &mut transcript,
        )?;

        // verify range proof
        let source_commitment = self.source_commitment.try_into()?;
        range_proof.verify(
            vec![
                &source_commitment,
                &ciphertext_lo.commitment,
                &ciphertext_hi.commitment,
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
pub(crate) struct TransferPubkeys {
    pub source: ElGamalPubkey,
    pub dest: ElGamalPubkey,
    pub auditor: ElGamalPubkey,
}

impl TransferPubkeys {
    pub fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[..32].copy_from_slice(&self.source.to_bytes());
        bytes[32..64].copy_from_slice(&self.dest.to_bytes());
        bytes[64..96].copy_from_slice(&self.auditor.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProofError> {
        let bytes = array_ref![bytes, 0, 96];
        let (source, dest, auditor) = array_refs![bytes, 32, 32, 32];

        let source = ElGamalPubkey::from_bytes(source).ok_or(ProofError::Verification)?;
        let dest = ElGamalPubkey::from_bytes(dest).ok_or(ProofError::Verification)?;
        let auditor = ElGamalPubkey::from_bytes(auditor).ok_or(ProofError::Verification)?;

        Ok(Self {
            source,
            dest,
            auditor,
        })
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
