use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            discrete_log::*,
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{Pedersen, PedersenCommitment, PedersenDecryptHandle, PedersenOpening},
        },
        equality_proof::EqualityProof,
        errors::ProofError,
        instruction::{Role, Verifiable},
        range_proof::RangeProof,
        transcript::TranscriptProtocol,
        validity_proof::ValidityProof,
    },
    curve25519_dalek::scalar::Scalar,
    merlin::Transcript,
    std::convert::TryInto,
};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferData {
    /// The transfer amount encoded as Pedersen commitments
    pub amount_comms: TransferCommitments,

    /// The decryption handles that allow decryption of the lo-bits of the transfer amount
    pub decrypt_handles_lo: TransferDecryptHandles,

    /// The decryption handles that allow decryption of the hi-bits of the transfer amount
    pub decrypt_handles_hi: TransferDecryptHandles,

    /// The public encryption keys associated with the transfer: source, dest, and auditor
    pub transfer_public_keys: TransferPubkeys, // 96 bytes

    /// The final spendable ciphertext after the transfer
    pub new_spendable_ct: pod::ElGamalCiphertext, // 64 bytes

    /// Zero-knowledge proofs for Transfer
    pub proof: TransferProof,
}

#[cfg(not(target_arch = "bpf"))]
impl TransferData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transfer_amount: u64,
        spendable_balance: u64,
        spendable_ct: ElGamalCiphertext,
        source_keypair: &ElGamalKeypair,
        dest_pk: ElGamalPubkey,
        auditor_pk: ElGamalPubkey,
    ) -> Self {
        // split and encrypt transfer amount
        //
        // encryption is a bit more involved since we are generating each components of an ElGamalKeypair
        // ciphertext separately.
        let (amount_lo, amount_hi) = split_u64_into_u32(transfer_amount);

        let (comm_lo, open_lo) = Pedersen::new(amount_lo);
        let (comm_hi, open_hi) = Pedersen::new(amount_hi);

        let handle_source_lo = source_keypair.public.decrypt_handle(&open_lo);
        let handle_dest_lo = dest_pk.decrypt_handle(&open_lo);
        let handle_auditor_lo = auditor_pk.decrypt_handle(&open_lo);

        let handle_source_hi = source_keypair.public.decrypt_handle(&open_hi);
        let handle_dest_hi = dest_pk.decrypt_handle(&open_hi);
        let handle_auditor_hi = auditor_pk.decrypt_handle(&open_hi);

        // message encoding as Pedersen commitments, which will be included in range proof data
        let amount_comms = TransferCommitments {
            lo: comm_lo.into(),
            hi: comm_hi.into(),
        };

        // decryption handles, which will be included in the validity proof data
        let decrypt_handles_lo = TransferDecryptHandles {
            source: handle_source_lo.into(),
            dest: handle_dest_lo.into(),
            auditor: handle_auditor_lo.into(),
        };

        let decrypt_handles_hi = TransferDecryptHandles {
            source: handle_source_hi.into(),
            dest: handle_dest_hi.into(),
            auditor: handle_auditor_hi.into(),
        };

        // grouping of the public keys for the transfer
        let transfer_public_keys = TransferPubkeys {
            source_pk: source_keypair.public.into(),
            dest_pk: dest_pk.into(),
            auditor_pk: auditor_pk.into(),
        };

        // subtract transfer amount from the spendable ciphertext
        let spendable_comm = spendable_ct.message_comm;
        let spendable_handle = spendable_ct.decrypt_handle;

        let new_spendable_balance = spendable_balance - transfer_amount;
        let new_spendable_comm = spendable_comm - combine_u32_comms(comm_lo, comm_hi);
        let new_spendable_handle =
            spendable_handle - combine_u32_handles(handle_source_lo, handle_source_hi);

        let new_spendable_ct = ElGamalCiphertext {
            message_comm: new_spendable_comm,
            decrypt_handle: new_spendable_handle,
        };

        // range_proof and validity_proof should be generated together
        let proof = TransferProof::new(
            source_keypair,
            &dest_pk,
            &auditor_pk,
            (amount_lo as u64, amount_hi as u64),
            (&open_lo, &open_hi),
            new_spendable_balance,
            &new_spendable_ct,
        );

        Self {
            amount_comms,
            decrypt_handles_lo,
            decrypt_handles_hi,
            new_spendable_ct: new_spendable_ct.into(),
            transfer_public_keys,
            proof,
        }
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_lo(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let transfer_comm_lo: PedersenCommitment = self.amount_comms.lo.try_into()?;

        let decryption_handle_lo = match role {
            Role::Source => self.decrypt_handles_lo.source,
            Role::Dest => self.decrypt_handles_lo.dest,
            Role::Auditor => self.decrypt_handles_lo.auditor,
        }
        .try_into()?;

        Ok((transfer_comm_lo, decryption_handle_lo).into())
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    fn ciphertext_hi(&self, role: Role) -> Result<ElGamalCiphertext, ProofError> {
        let transfer_comm_hi: PedersenCommitment = self.amount_comms.hi.try_into()?;

        let decryption_handle_hi = match role {
            Role::Source => self.decrypt_handles_hi.source,
            Role::Dest => self.decrypt_handles_hi.dest,
            Role::Auditor => self.decrypt_handles_hi.auditor,
        }
        .try_into()?;

        Ok((transfer_comm_hi, decryption_handle_hi).into())
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
            Err(ProofError::VerificationError)
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for TransferData {
    fn verify(&self) -> Result<(), ProofError> {
        self.proof.verify(
            &self.amount_comms,
            &self.decrypt_handles_lo,
            &self.decrypt_handles_hi,
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
    pub validity_proof: pod::ValidityProof,

    // Associated range proof
    pub range_proof: pod::RangeProof128,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"TransferProof")
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::many_single_char_names)]
    pub fn new(
        source_keypair: &ElGamalKeypair,
        dest_pk: &ElGamalPubkey,
        auditor_pk: &ElGamalPubkey,
        transfer_amt: (u64, u64),
        openings: (&PedersenOpening, &PedersenOpening),
        source_new_balance: u64,
        source_new_balance_ct: &ElGamalCiphertext,
    ) -> Self {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.transfer_proof_domain_sep();

        // generate a Pedersen commitment for the remaining balance in source
        let (source_commitment, source_open) = Pedersen::new(source_new_balance);

        // extract the relevant scalar and Ristretto points from the inputs
        let P_EG = source_keypair.public.get_point();
        let C_EG = source_new_balance_ct.message_comm.get_point();
        let D_EG = source_new_balance_ct.decrypt_handle.get_point();
        let C_Ped = source_commitment.get_point();

        // append all current state to the transcript
        transcript.append_point(b"P_EG", &P_EG.compress());
        transcript.append_point(b"C_EG", &C_EG.compress());
        transcript.append_point(b"D_EG", &D_EG.compress());
        transcript.append_point(b"C_Ped", &C_Ped.compress());

        // let c = transcript.challenge_scalar(b"c");
        // println!("{:?}", c);

        // generate equality_proof
        let equality_proof = EqualityProof::new(
            source_keypair,
            source_new_balance_ct,
            source_new_balance,
            &source_open,
            &mut transcript,
        );

        // generate ciphertext validity proof
        let validity_proof =
            ValidityProof::new(dest_pk, auditor_pk, transfer_amt, openings, &mut transcript);

        // generate the range proof
        let range_proof = RangeProof::create(
            vec![source_new_balance, transfer_amt.0, transfer_amt.1],
            vec![64, 32, 32],
            vec![&source_open, openings.0, openings.1],
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
        let validity_proof: ValidityProof = self.validity_proof.try_into()?;
        let range_proof: RangeProof = self.range_proof.try_into()?;

        // add a domain separator to record the start of the protocol
        transcript.transfer_proof_domain_sep();

        // extract the relevant scalar and Ristretto points from the inputs
        let source_pk: ElGamalPubkey = transfer_public_keys.source_pk.try_into()?;
        let new_spendable_ct: ElGamalCiphertext = (*new_spendable_ct).try_into()?;

        let P_EG = source_pk.get_point();
        let C_EG = new_spendable_ct.message_comm.get_point();
        let D_EG = new_spendable_ct.decrypt_handle.get_point();
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

        let handle_lo_dest: PedersenDecryptHandle = decryption_handles_lo.dest.try_into()?;
        let handle_hi_dest: PedersenDecryptHandle = decryption_handles_hi.dest.try_into()?;

        let handle_lo_auditor: PedersenDecryptHandle = decryption_handles_lo.auditor.try_into()?;
        let handle_hi_auditor: PedersenDecryptHandle = decryption_handles_hi.auditor.try_into()?;

        // TODO: validity proof
        validity_proof.verify(
            &dest_elgamal_pubkey,
            &auditor_elgamal_pubkey,
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
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubkeys {
    pub source_pk: pod::ElGamalPubkey,  // 32 bytes
    pub dest_pk: pod::ElGamalPubkey,    // 32 bytes
    pub auditor_pk: pod::ElGamalPubkey, // 32 bytes
}

/// The transfer amount commitments needed for a transfer
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferCommitments {
    pub lo: pod::PedersenCommitment, // 32 bytes
    pub hi: pod::PedersenCommitment, // 32 bytes
}

/// The decryption handles needed for a transfer
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferDecryptHandles {
    pub source: pod::PedersenDecryptHandle,  // 32 bytes
    pub dest: pod::PedersenDecryptHandle,    // 32 bytes
    pub auditor: pod::PedersenDecryptHandle, // 32 bytes
}

/// Split u64 number into two u32 numbers
#[cfg(not(target_arch = "bpf"))]
pub fn split_u64_into_u32(amt: u64) -> (u32, u32) {
    let lo = amt as u32;
    let hi = (amt >> 32) as u32;

    (lo, hi)
}

/// Constant for 2^32
#[cfg(not(target_arch = "bpf"))]
const TWO_32: u64 = 4294967296;

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_comms(
    comm_lo: PedersenCommitment,
    comm_hi: PedersenCommitment,
) -> PedersenCommitment {
    comm_lo + comm_hi * Scalar::from(TWO_32)
}

#[cfg(not(target_arch = "bpf"))]
pub fn combine_u32_handles(
    handle_lo: PedersenDecryptHandle,
    handle_hi: PedersenDecryptHandle,
) -> PedersenDecryptHandle {
    handle_lo + handle_hi * Scalar::from(TWO_32)
}

/*
pub fn combine_u32_ciphertexts(ct_lo: ElGamalCiphertext, ct_hi: ElGamalCiphertext) -> ElGamalCiphertext {
    ct_lo + ct_hi * Scalar::from(TWO_32)
}*/

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_transfer_correctness() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let source_keypair = ElGamalKeypair::default();
        let dest_pk = ElGamalKeypair::default().public;
        let auditor_pk = ElGamalKeypair::default().public;

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
        let source_keypair = ElGamalKeypair::default();

        let ElGamalKeypair {
            public: dest_pk,
            secret: dest_sk,
        } = ElGamalKeypair::default();

        let ElGamalKeypair {
            public: auditor_pk,
            secret: auditor_sk,
        } = ElGamalKeypair::default();

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
