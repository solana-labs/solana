use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::{
            elgamal::{ElGamalCiphertext, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{
                Pedersen, PedersenBase, PedersenCommitment, PedersenDecryptHandle, PedersenOpening,
            },
            discrete_log::*,
        },
        errors::ProofError,
        instruction::Verifiable,
        range_proof::RangeProof,
        transcript::TranscriptProtocol,
    },
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, MultiscalarMul, VartimeMultiscalarMul},
    },
    merlin::Transcript,
    rand::rngs::OsRng,
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
    pub transfer_public_keys: TransferPubKeys, // 96 bytes

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
        source_pk: ElGamalPubkey,
        source_sk: &ElGamalSecretKey,
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

        let handle_source_lo = source_pk.decrypt_handle(&open_lo);
        let handle_dest_lo = dest_pk.decrypt_handle(&open_lo);
        let handle_auditor_lo = auditor_pk.decrypt_handle(&open_lo);

        let handle_source_hi = source_pk.decrypt_handle(&open_hi);
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
        let transfer_public_keys = TransferPubKeys {
            source_pk: source_pk.into(),
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
            source_sk,
            &source_pk,
            &dest_pk,
            &auditor_pk,
            (amount_lo as u64, amount_hi as u64),
            &open_lo,
            &open_hi,
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
    pub fn ciphertext_lo(&self, role: &TransferRole) -> Result<ElGamalCiphertext, ProofError> {
        let transfer_comm_lo: PedersenCommitment = self.amount_comms.lo.try_into()?;

        let decryption_handle_lo = match role {
            TransferRole::Source => self.decrypt_handles_lo.source,
            TransferRole::Dest => self.decrypt_handles_lo.dest,
            TransferRole::Auditor => self.decrypt_handles_lo.auditor,
        }.try_into()?;

        Ok((transfer_comm_lo, decryption_handle_lo).into())
    }

    /// Extracts the lo ciphertexts associated with a transfer data
    pub fn ciphertext_hi(&self, role: &TransferRole) -> Result<ElGamalCiphertext, ProofError> {
        let transfer_comm_hi: PedersenCommitment = self.amount_comms.hi.try_into()?;

        let decryption_handle_hi = match role {
            TransferRole::Source => self.decrypt_handles_hi.source,
            TransferRole::Dest => self.decrypt_handles_hi.dest,
            TransferRole::Auditor => self.decrypt_handles_hi.auditor,
        }.try_into()?;

        Ok((transfer_comm_hi, decryption_handle_hi).into())
    }

    /// Decrypts transfer amount from transfer data
    ///
    /// TODO: This function should run in constant time. Use `subtle::Choice` for the if statement
    /// and make sure that the function does not terminate prematurely due to errors
    pub fn decrypt_amount(&self, role: &TransferRole, sk: &ElGamalSecretKey) -> Result<u64, ProofError> {
        let ciphertext_lo = self.ciphertext_lo(role)?;
        let ciphertext_hi = self.ciphertext_hi(role)?;

        let amount_lo = ciphertext_lo.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);
        let amount_hi = ciphertext_hi.decrypt_u32_online(sk, &DECODE_U32_PRECOMPUTATION_FOR_G);

        if amount_lo.is_some() && amount_hi.is_some() {
            // Will panic if overflown
            Ok((amount_lo.unwrap() as u64) + (TWO_32 * amount_hi.unwrap() as u64))
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
    // Proof component for the spendable ciphertext components: R
    pub R: pod::CompressedRistretto, // 32 bytes
    // Proof component for the spendable ciphertext components: z
    pub z: pod::Scalar, // 32 bytes
    // Proof component for the transaction amount components: T_src
    pub T_joint: pod::CompressedRistretto, // 32 bytes
    // Proof component for the transaction amount components: T_1
    pub T_1: pod::CompressedRistretto, // 32 bytes
    // Proof component for the transaction amount components: T_2
    pub T_2: pod::CompressedRistretto, // 32 bytes
    // Range proof component
    pub range_proof: pod::RangeProof128,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferProof {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::many_single_char_names)]
    pub fn new(
        source_sk: &ElGamalSecretKey,
        source_pk: &ElGamalPubkey,
        dest_pk: &ElGamalPubkey,
        auditor_pk: &ElGamalPubkey,
        transfer_amt: (u64, u64),
        lo_open: &PedersenOpening,
        hi_open: &PedersenOpening,
        new_spendable_balance: u64,
        new_spendable_ct: &ElGamalCiphertext,
    ) -> Self {
        // TODO: should also commit to pubkeys and commitments later
        let mut transcript = merlin::Transcript::new(b"TransferProof");

        let H = PedersenBase::default().H;
        let D = new_spendable_ct.decrypt_handle.get_point();
        let s = source_sk.get_scalar();

        // Generate proof for the new spendable ciphertext
        let r_new = Scalar::random(&mut OsRng);
        let y = Scalar::random(&mut OsRng);
        let R = RistrettoPoint::multiscalar_mul(vec![y, r_new], vec![D, H]).compress();

        transcript.append_point(b"R", &R);
        let c = transcript.challenge_scalar(b"c");

        let z = s + c * y;
        let new_spendable_open = PedersenOpening(c * r_new);

        // Generate proof for the transfer amounts
        let t_1_blinding = PedersenOpening::random(&mut OsRng);
        let t_2_blinding = PedersenOpening::random(&mut OsRng);

        // generate the range proof
        let range_proof = RangeProof::create_with(
            vec![new_spendable_balance, transfer_amt.0, transfer_amt.1],
            vec![64, 32, 32],
            vec![&new_spendable_open, lo_open, hi_open],
            &t_1_blinding,
            &t_2_blinding,
            &mut transcript,
        );

        let u = transcript.challenge_scalar(b"u");

        let P_joint = RistrettoPoint::multiscalar_mul(
            vec![Scalar::one(), u, u * u],
            vec![
                source_pk.get_point(),
                dest_pk.get_point(),
                auditor_pk.get_point(),
            ],
        );
        let T_joint = (new_spendable_open.get_scalar() * P_joint).compress();
        let T_1 = (t_1_blinding.get_scalar() * P_joint).compress();
        let T_2 = (t_2_blinding.get_scalar() * P_joint).compress();

        transcript.append_point(b"T_1", &T_1);
        transcript.append_point(b"T_2", &T_2);

        Self {
            R: R.into(),
            z: z.into(),
            T_joint: T_joint.into(),
            T_1: T_1.into(),
            T_2: T_2.into(),
            range_proof: range_proof.try_into().expect("invalid range proof"),
        }
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl TransferProof {
    pub fn verify(
        self,
        amount_comms: &TransferCommitments,
        decryption_handles_lo: &TransferDecryptHandles,
        decryption_handles_hi: &TransferDecryptHandles,
        new_spendable_ct: &pod::ElGamalCiphertext,
        transfer_public_keys: &TransferPubKeys,
    ) -> Result<(), ProofError> {
        let mut transcript = Transcript::new(b"TransferProof");

        let range_proof: RangeProof = self.range_proof.try_into()?;

        let source_pk: ElGamalPubkey = transfer_public_keys.source_pk.try_into()?;
        let dest_pk: ElGamalPubkey = transfer_public_keys.dest_pk.try_into()?;
        let auditor_pk: ElGamalPubkey = transfer_public_keys.auditor_pk.try_into()?;

        // derive Pedersen commitment for range proof verification
        let new_spendable_ct: ElGamalCiphertext = (*new_spendable_ct).try_into()?;

        let C = new_spendable_ct.message_comm.get_point();
        let D = new_spendable_ct.decrypt_handle.get_point();

        let R = self.R.into();
        let z: Scalar = self.z.into();

        transcript.validate_and_append_point(b"R", &R)?;
        let c = transcript.challenge_scalar(b"c");

        let R = R.decompress().ok_or(ProofError::VerificationError)?;

        let spendable_comm_verification =
            RistrettoPoint::multiscalar_mul(vec![Scalar::one(), -z, c], vec![C, D, R]).compress();

        // verify range proof
        let range_proof_verification = range_proof.verify_challenges(
            vec![
                &spendable_comm_verification,
                &amount_comms.lo.into(),
                &amount_comms.hi.into(),
            ],
            vec![64_usize, 32_usize, 32_usize],
            &mut transcript,
        );

        if range_proof_verification.is_err() {
            return Err(ProofError::VerificationError);
        }
        let (z, x) = range_proof_verification.unwrap();

        // check well-formedness of decryption handles

        // derive joint public key
        let u = transcript.challenge_scalar(b"u");
        let P_joint = RistrettoPoint::vartime_multiscalar_mul(
            vec![Scalar::one(), u, u * u],
            vec![
                source_pk.get_point(),
                dest_pk.get_point(),
                auditor_pk.get_point(),
            ],
        );

        let t_x_blinding: Scalar = range_proof.t_x_blinding;
        let T_1: CompressedRistretto = self.T_1.into();
        let T_2: CompressedRistretto = self.T_2.into();

        let handle_source_lo: PedersenDecryptHandle = decryption_handles_lo.source.try_into()?;
        let handle_dest_lo: PedersenDecryptHandle = decryption_handles_lo.dest.try_into()?;
        let handle_auditor_lo: PedersenDecryptHandle = decryption_handles_lo.auditor.try_into()?;

        let D_joint: CompressedRistretto = self.T_joint.into();
        let D_joint = D_joint.decompress().ok_or(ProofError::VerificationError)?;

        let D_joint_lo = RistrettoPoint::vartime_multiscalar_mul(
            vec![Scalar::one(), u, u * u],
            vec![
                handle_source_lo.get_point(),
                handle_dest_lo.get_point(),
                handle_auditor_lo.get_point(),
            ],
        );

        let handle_source_hi: PedersenDecryptHandle = decryption_handles_hi.source.try_into()?;
        let handle_dest_hi: PedersenDecryptHandle = decryption_handles_hi.dest.try_into()?;
        let handle_auditor_hi: PedersenDecryptHandle = decryption_handles_hi.auditor.try_into()?;

        let D_joint_hi = RistrettoPoint::vartime_multiscalar_mul(
            vec![Scalar::one(), u, u * u],
            vec![
                handle_source_hi.get_point(),
                handle_dest_hi.get_point(),
                handle_auditor_hi.get_point(),
            ],
        );

        // TODO: combine Pedersen commitment verification above to here for efficiency
        // TODO: might need to add an additional proof-of-knowledge here (additional 64 byte)
        let mega_check = RistrettoPoint::optional_multiscalar_mul(
            vec![-t_x_blinding, x, x * x, z * z, z * z * z, z * z * z * z],
            vec![
                Some(P_joint),
                T_1.decompress(),
                T_2.decompress(),
                Some(D_joint),
                Some(D_joint_lo),
                Some(D_joint_hi),
            ],
        )
        .ok_or(ProofError::VerificationError)?;

        if mega_check.is_identity() {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
        }
    }
}

/// The ElGamal public keys needed for a transfer
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct TransferPubKeys {
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

#[cfg(not(target_arch = "bpf"))]
pub enum TransferRole {
    Source,
    Dest,
    Auditor,
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
    use super::*;
    use crate::encryption::{discrete_log, elgamal::ElGamalKeypair};

    #[test]
    fn test_transfer_correctness() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let ElGamalKeypair {
            public: source_pk,
            secret: source_sk,
        } = ElGamalKeypair::default();
        let dest_pk = ElGamalKeypair::default().public;
        let auditor_pk = ElGamalKeypair::default().public;

        // create source account spendable ciphertext
        let spendable_balance: u64 = 77;
        let spendable_ct = source_pk.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            spendable_balance,
            spendable_ct,
            source_pk,
            &source_sk,
            dest_pk,
            auditor_pk,
        );

        assert!(transfer_data.verify().is_ok());
    }

    #[test]
    fn test_source_dest_ciphertext() {
        // ElGamalKeypair keys for source, destination, and auditor accounts
        let ElGamalKeypair {
            public: source_pk,
            secret: source_sk,
        } = ElGamalKeypair::default();

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
        let spendable_ct = source_pk.encrypt(spendable_balance);

        // transfer amount
        let transfer_amount: u64 = 55;

        // create transfer data
        let transfer_data = TransferData::new(
            transfer_amount,
            spendable_balance,
            spendable_ct,
            source_pk,
            &source_sk,
            dest_pk,
            auditor_pk,
        );

        assert_eq!(
            transfer_data.decrypt_amount(&TransferRole::Source, &source_sk).unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data.decrypt_amount(&TransferRole::Dest, &dest_sk).unwrap(),
            55_u64,
        );

        assert_eq!(
            transfer_data.decrypt_amount(&TransferRole::Auditor, &auditor_sk).unwrap(),
            55_u64,
        );
    }
}
