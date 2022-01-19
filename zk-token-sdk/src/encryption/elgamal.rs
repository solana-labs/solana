//! TODO

use {
    crate::encryption::{
        discrete_log::{DecodeU32Precomputation, DiscreteLog},
        pedersen::{Pedersen, PedersenCommitment, PedersenOpening, G, H},
    },
    arrayref::{array_ref, array_refs},
    core::ops::{Add, Mul, Sub},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
    },
    serde::{Deserialize, Serialize},
    solana_sdk::{
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        signature::Signature,
        signer::{Signer, SignerError},
    },
    std::convert::TryInto,
    subtle::{Choice, ConstantTimeEq},
    zeroize::Zeroize,
};
#[cfg(not(target_arch = "bpf"))]
use {
    rand::rngs::OsRng,
    sha3::Sha3_512,
    std::{
        fmt,
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::Path,
    },
};

/// Algorithm handle for the twisted ElGamal encryption scheme
struct ElGamal;
impl ElGamal {
    /// Generates an ElGamal keypair.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    fn keygen() -> ElGamalKeypair {
        // secret scalar should be zero with negligible probability
        let s = Scalar::random(&mut OsRng);
        let keypair = Self::keygen_with_scalar(&s);

        s.zeroize();
        keypair
    }

    /// Generates an ElGamal keypair from a scalar input that determines the ElGamal private key.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    fn keygen_with_scalar(s: &Scalar) -> ElGamalKeypair {
        assert!(s != &Scalar::zero());

        let P = s.invert() * H;

        ElGamalKeypair {
            public: ElGamalPubkey(P),
            secret: ElGamalSecretKey(*s),
        }
    }

    /// On input an ElGamal public key and a mesage to be encrypted, the function returns a
    /// corresponding ElGamal ciphertext.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_arch = "bpf"))]
    fn encrypt<T: Into<Scalar>>(public: &ElGamalPubkey, amount: T) -> ElGamalCiphertext {
        let (commitment, opening) = Pedersen::new(amount);
        let handle = public.decrypt_handle(&opening);

        ElGamalCiphertext { commitment, handle }
    }

    /// On input a public key, message, and Pedersen opening, the function
    /// returns the corresponding ElGamal ciphertext.
    fn encrypt_with<T: Into<Scalar>>(
        amount: T,
        public: &ElGamalPubkey,
        opening: &PedersenOpening,
    ) -> ElGamalCiphertext {
        let commitment = Pedersen::with(amount, opening);
        let handle = public.decrypt_handle(opening);

        ElGamalCiphertext { commitment, handle }
    }

    /// On input a secret key and a ciphertext, the function returns the decrypted message.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover, the originally encrypted
    /// message, use `DiscreteLog::decode`.
    fn decrypt(secret: &ElGamalSecretKey, ciphertext: &ElGamalCiphertext) -> DiscreteLog {
        DiscreteLog {
            generator: G,
            target: &ciphertext.commitment.0 - &(&secret.0 * &ciphertext.handle.0),
        }
    }

    /// On input a secret key and a ciphertext, the function returns the decrypted message
    /// interpretted as type `u32`.
    fn decrypt_u32(secret: &ElGamalSecretKey, ciphertext: &ElGamalCiphertext) -> Option<u32> {
        let discrete_log_instance = Self::decrypt(secret, ciphertext);
        discrete_log_instance.decode_u32()
    }

    /// On input a secret key, a ciphertext, and a pre-computed hashmap, the function returns the
    /// decrypted message interpretted as type `u32`.
    fn decrypt_u32_online(
        secret: &ElGamalSecretKey,
        ciphertext: &ElGamalCiphertext,
        hashmap: &DecodeU32Precomputation,
    ) -> Option<u32> {
        let discrete_log_instance = Self::decrypt(secret, ciphertext);
        discrete_log_instance.decode_u32_online(hashmap)
    }
}

/// A (twisted) ElGamal encryption keypair.
///
/// The instances of the secret key are zeroized on drop.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Zeroize)]
pub struct ElGamalKeypair {
    /// The public half of this keypair.
    pub public: ElGamalPubkey,
    /// The secret half of this keypair.
    pub secret: ElGamalSecretKey,
}

impl ElGamalKeypair {
    /// Deterministically derives an ElGamal keypair from an Ed25519 signing key and a Solana address.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    pub fn new(signer: &dyn Signer, address: &Pubkey) -> Result<Self, SignerError> {
        let message = Message::new(
            &[Instruction::new_with_bytes(
                *address,
                b"ElGamalSecretKey",
                vec![],
            )],
            Some(&signer.try_pubkey()?),
        );
        let signature = signer.try_sign_message(&message.serialize())?;

        // Some `Signer` implementations return the default signature, which is not suitable for
        // use as key material
        if signature == Signature::default() {
            return Err(SignerError::Custom("Rejecting default signature".into()));
        }

        let scalar = Scalar::hash_from_bytes::<Sha3_512>(signature.as_ref());
        let keypair = ElGamal::keygen_with_scalar(&scalar);

        // TODO: zeroize signature?
        scalar.zeroize();
        Ok(keypair)
    }

    /// Generates the public and secret keys for ElGamal encryption.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(clippy::new_ret_no_self)]
    pub fn random() -> Self {
        ElGamal::keygen()
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(self.public.as_bytes());
        bytes[32..].copy_from_slice(self.secret.as_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            public: ElGamalPubkey::from_bytes(bytes[..32].try_into().ok()?)?,
            secret: ElGamalSecretKey::from_bytes(bytes[32..].try_into().ok()?)?,
        })
    }

    /// Reads a JSON-encoded keypair from a `Reader` implementor
    pub fn read_json<R: Read>(reader: &mut R) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes: Vec<u8> = serde_json::from_reader(reader)?;
        Self::from_bytes(&bytes).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Invalid ElGamalKeypair").into()
        })
    }

    /// Reads keypair from a file
    pub fn read_json_file<F: AsRef<Path>>(path: F) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(path.as_ref())?;
        Self::read_json(&mut file)
    }

    /// Writes to a `Write` implementer with JSON-encoding
    pub fn write_json<W: Write>(
        &self,
        writer: &mut W,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let bytes = self.to_bytes();
        let json = serde_json::to_string(&bytes.to_vec())?;
        writer.write_all(&json.clone().into_bytes())?;
        Ok(json)
    }

    /// Write keypair to a file with JSON-encoding
    pub fn write_json_file<F: AsRef<Path>>(
        &self,
        outfile: F,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let outfile = outfile.as_ref();

        if let Some(outdir) = outfile.parent() {
            fs::create_dir_all(outdir)?;
        }

        let mut f = {
            #[cfg(not(unix))]
            {
                OpenOptions::new()
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                OpenOptions::new().mode(0o600)
            }
        }
        .write(true)
        .truncate(true)
        .create(true)
        .open(outfile)?;

        self.write_json(&mut f)
    }
}

/// Public key for the ElGamal encryption scheme.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Zeroize)]
pub struct ElGamalPubkey(RistrettoPoint);
impl ElGamalPubkey {
    /// Derives the `ElGamalPubkey` that uniquely corresponds to an `ElGamalSecretKey`.
    #[allow(non_snake_case)]
    pub fn new(secret: &ElGamalSecretKey) -> Self {
        ElGamalPubkey(&secret.0 * &H)
    }

    pub fn get_point(&self) -> &RistrettoPoint {
        &self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.compress().as_bytes()
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Option<ElGamalPubkey> {
        Some(ElGamalPubkey(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }

    /// Encrypts an amount under the public key.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_arch = "bpf"))]
    pub fn encrypt<T: Into<Scalar>>(&self, amount: T) -> ElGamalCiphertext {
        ElGamal::encrypt(self, amount)
    }

    /// Encrypts an amount under the public key and an input Pedersen opening.
    pub fn encrypt_with<T: Into<Scalar>>(
        &self,
        amount: T,
        opening: &PedersenOpening,
    ) -> ElGamalCiphertext {
        ElGamal::encrypt_with(amount, self, opening)
    }

    /// Generates a decryption handle for an ElGamal public key under a Pedersen
    /// opening.
    pub fn decrypt_handle(self, opening: &PedersenOpening) -> DecryptHandle {
        DecryptHandle::new(&self, opening)
    }
}

impl fmt::Display for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", base64::encode(self.to_bytes()))
    }
}

/// Secret key for the ElGamal encryption scheme.
///
/// Instances of ElGamal secret key are zeroized on drop.
#[derive(Clone, Debug, Deserialize, Serialize, Zeroize)]
#[zeroize(drop)]
pub struct ElGamalSecretKey(Scalar);
impl ElGamalSecretKey {
    /// Deterministically derives an ElGamal keypair from an Ed25519 signing key and a Solana
    /// address.
    pub fn new(signer: &dyn Signer, address: &Pubkey) -> Result<Self, SignerError> {
        let message = Message::new(
            &[Instruction::new_with_bytes(
                *address,
                b"ElGamalSecretKey",
                vec![],
            )],
            Some(&signer.try_pubkey()?),
        );
        let signature = signer.try_sign_message(&message.serialize())?;

        // Some `Signer` implementations return the default signature, which is not suitable for
        // use as key material
        if signature == Signature::default() {
            Err(SignerError::Custom("Rejecting default signature".into()))
        } else {
            Ok(ElGamalSecretKey(Scalar::hash_from_bytes::<Sha3_512>(
                signature.as_ref(),
            )))
        }
    }

    /// Randomly samples an ElGamal secret key.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    pub fn random() -> Self {
        ElGamalSecretKey(Scalar::random(&mut OsRng))
    }

    pub fn get_scalar(&self) -> &Scalar {
        &self.0
    }

    /// Decrypts a ciphertext using the ElGamal secret key.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover, the originally encrypted
    /// message, use `DiscreteLog::decode`.
    pub fn decrypt(&self, ciphertext: &ElGamalCiphertext) -> DiscreteLog {
        ElGamal::decrypt(self, ciphertext)
    }

    /// Decrypts a ciphertext using the ElGamal secret key interpretting the message as type `u32`.
    pub fn decrypt_u32(&self, ciphertext: &ElGamalCiphertext) -> Option<u32> {
        ElGamal::decrypt_u32(self, ciphertext)
    }

    /// Decrypts a ciphertext using the ElGamal secret key and a pre-computed hashmap. It
    /// interprets the decrypted message as type `u32`.
    pub fn decrypt_u32_online(
        &self,
        ciphertext: &ElGamalCiphertext,
        hashmap: &DecodeU32Precomputation,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(self, ciphertext, hashmap)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Option<ElGamalSecretKey> {
        Scalar::from_canonical_bytes(bytes).map(ElGamalSecretKey)
    }
}

impl From<Scalar> for ElGamalSecretKey {
    fn from(scalar: Scalar) -> ElGamalSecretKey {
        ElGamalSecretKey(scalar)
    }
}

impl Eq for ElGamalSecretKey {}
impl PartialEq for ElGamalSecretKey {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).unwrap_u8() == 1u8
    }
}
impl ConstantTimeEq for ElGamalSecretKey {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

/// Ciphertext for the ElGamal encryption scheme.
#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ElGamalCiphertext {
    pub commitment: PedersenCommitment,
    pub handle: DecryptHandle,
}
impl ElGamalCiphertext {
    pub fn add_msg<T: Into<Scalar>>(&self, amount: T) -> Self {
        let commitment_to_add = PedersenCommitment(amount.into() * G);
        ElGamalCiphertext {
            commitment: &self.commitment + &commitment_to_add,
            handle: self.handle,
        }
    }

    pub fn subtract_msg<T: Into<Scalar>>(&self, amount: T) -> Self {
        let commitment_to_subtract = PedersenCommitment(amount.into() * G);
        ElGamalCiphertext {
            commitment: &self.commitment - &commitment_to_subtract,
            handle: self.handle,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(self.commitment.as_bytes());
        bytes[32..].copy_from_slice(self.handle.as_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalCiphertext> {
        let bytes = array_ref![bytes, 0, 64];
        let (commitment, handle) = array_refs![bytes, 32, 32];

        let commitment = CompressedRistretto::from_slice(commitment).decompress()?;
        let handle = CompressedRistretto::from_slice(handle).decompress()?;

        Some(ElGamalCiphertext {
            commitment: PedersenCommitment(commitment),
            handle: DecryptHandle(handle),
        })
    }

    /// Decrypts the ciphertext using an ElGamal secret key.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover, the originally encrypted
    /// message, use `DiscreteLog::decode`.
    pub fn decrypt(&self, secret: &ElGamalSecretKey) -> DiscreteLog {
        ElGamal::decrypt(secret, self)
    }

    /// Decrypts the ciphertext using an ElGamal secret key interpretting the message as type `u32`.
    pub fn decrypt_u32(&self, secret: &ElGamalSecretKey) -> Option<u32> {
        ElGamal::decrypt_u32(secret, self)
    }

    /// Decrypts the ciphertext using an ElGamal secret key and a pre-computed hashmap. It
    /// interprets the decrypted message as type `u32`.
    pub fn decrypt_u32_online(
        &self,
        secret: &ElGamalSecretKey,
        hashmap: &DecodeU32Precomputation,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(secret, self, hashmap)
    }
}

impl<'a, 'b> Add<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn add(self, other: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment + &other.commitment,
            handle: &self.handle + &other.handle,
        }
    }
}

define_add_variants!(
    LHS = ElGamalCiphertext,
    RHS = ElGamalCiphertext,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Sub<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn sub(self, other: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment - &other.commitment,
            handle: &self.handle - &other.handle,
        }
    }
}

define_sub_variants!(
    LHS = ElGamalCiphertext,
    RHS = ElGamalCiphertext,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Mul<&'b Scalar> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn mul(self, other: &'b Scalar) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment * other,
            handle: &self.handle * other,
        }
    }
}

define_mul_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

/// Decryption handle for Pedersen commitment.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DecryptHandle(RistrettoPoint);
impl DecryptHandle {
    pub fn new(public: &ElGamalPubkey, opening: &PedersenOpening) -> Self {
        Self(&public.0 * &opening.0)
    }

    pub fn get_point(&self) -> &RistrettoPoint {
        &self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.compress().as_bytes()
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<DecryptHandle> {
        Some(DecryptHandle(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }
}

impl<'a, 'b> Add<&'b DecryptHandle> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn add(self, other: &'b DecryptHandle) -> DecryptHandle {
        DecryptHandle(&self.0 + &other.0)
    }
}

define_add_variants!(
    LHS = DecryptHandle,
    RHS = DecryptHandle,
    Output = DecryptHandle
);

impl<'a, 'b> Sub<&'b DecryptHandle> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn sub(self, other: &'b DecryptHandle) -> DecryptHandle {
        DecryptHandle(&self.0 - &other.0)
    }
}

define_sub_variants!(
    LHS = DecryptHandle,
    RHS = DecryptHandle,
    Output = DecryptHandle
);

impl<'a, 'b> Mul<&'b Scalar> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn mul(self, other: &'b Scalar) -> DecryptHandle {
        DecryptHandle(&self.0 * other)
    }
}

define_mul_variants!(LHS = DecryptHandle, RHS = Scalar, Output = DecryptHandle);

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::encryption::pedersen::Pedersen,
        solana_sdk::{signature::Keypair, signer::null_signer::NullSigner},
    };

    #[test]
    fn test_encrypt_decrypt_correctness() {
        let ElGamalKeypair { public, secret } = ElGamalKeypair::random();
        let amount: u32 = 57;
        let ct = ElGamal::encrypt(&public, amount);

        let expected_instance = DiscreteLog {
            generator: G,
            target: Scalar::from(amount) * G,
        };

        assert_eq!(expected_instance, ElGamal::decrypt(&secret, &ct));

        // Commenting out for faster testing
        // assert_eq!(msg, ElGamalKeypair::decrypt_u32(&secret, &ct).unwrap());
    }

    #[test]
    fn test_decrypt_handle() {
        let ElGamalKeypair {
            public: pk_1,
            secret: sk_1,
        } = ElGamalKeypair::random();
        let ElGamalKeypair {
            public: pk_2,
            secret: sk_2,
        } = ElGamalKeypair::random();

        let msg: u32 = 77;
        let (comm, open) = Pedersen::new(msg);

        let decrypt_handle_1 = pk_1.decrypt_handle(&open);
        let decrypt_handle_2 = pk_2.decrypt_handle(&open);

        let ct_1: ElGamalCiphertext = (comm, decrypt_handle_1).into();
        let ct_2: ElGamalCiphertext = (comm, decrypt_handle_2).into();

        let expected_instance = DiscreteLog {
            generator: G,
            target: Scalar::from(msg) * G,
        };

        assert_eq!(expected_instance, sk_1.decrypt(&ct_1));
        assert_eq!(expected_instance, sk_2.decrypt(&ct_2));

        // Commenting out for faster testing
        // assert_eq!(msg, sk_1.decrypt_u32(&ct_1).unwrap());
        // assert_eq!(msg, sk_2.decrypt_u32(&ct_2).unwrap());
    }

    #[test]
    fn test_homomorphic_addition() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();
        let msg_0: u64 = 57;
        let msg_1: u64 = 77;

        // Add two ElGamal ciphertexts
        let open_0 = PedersenOpening::random(&mut OsRng);
        let open_1 = PedersenOpening::random(&mut OsRng);

        let ct_0 = ElGamal::encrypt_with(&public, msg_0, &open_0);
        let ct_1 = ElGamal::encrypt_with(&public, msg_1, &open_1);

        let ct_sum = ElGamal::encrypt_with(&public, msg_0 + msg_1, &(open_0 + open_1));

        assert_eq!(ct_sum, ct_0 + ct_1);

        // Add to ElGamal ciphertext
        let open = PedersenOpening::random(&mut OsRng);
        let ct = ElGamal::encrypt_with(&public, msg_0, &open);
        let ct_sum = ElGamal::encrypt_with(&public, msg_0 + msg_1, &open);

        assert_eq!(ct_sum, ct.add_to_msg(msg_1));
    }

    #[test]
    fn test_homomorphic_subtraction() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();
        let msg_0: u64 = 77;
        let msg_1: u64 = 55;

        // Subtract two ElGamal ciphertexts
        let open_0 = PedersenOpening::random(&mut OsRng);
        let open_1 = PedersenOpening::random(&mut OsRng);

        let ct_0 = ElGamal::encrypt_with(&public, msg_0, &open_0);
        let ct_1 = ElGamal::encrypt_with(&public, msg_1, &open_1);

        let ct_sub = ElGamal::encrypt_with(&public, msg_0 - msg_1, &(open_0 - open_1));

        assert_eq!(ct_sub, ct_0 - ct_1);

        // Subtract to ElGamal ciphertext
        let open = PedersenOpening::random(&mut OsRng);
        let ct = ElGamal::encrypt_with(&public, msg_0, &open);
        let ct_sub = ElGamal::encrypt_with(&public, msg_0 - msg_1, &open);

        assert_eq!(ct_sub, ct.sub_to_msg(msg_1));
    }

    #[test]
    fn test_homomorphic_multiplication() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();
        let msg_0: u64 = 57;
        let msg_1: u64 = 77;

        let open = PedersenOpening::random(&mut OsRng);

        let ct = ElGamal::encrypt_with(&public, msg_0, &open);
        let scalar = Scalar::from(msg_1);

        let ct_prod = ElGamal::encrypt_with(&public, msg_0 * msg_1, &(open * scalar));

        assert_eq!(ct_prod, ct * scalar);
    }

    #[test]
    fn test_homomorphic_division() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();
        let msg_0: u64 = 55;
        let msg_1: u64 = 5;

        let open = PedersenOpening::random(&mut OsRng);

        let ct = ElGamal::encrypt_with(&public, msg_0, &open);
        let scalar = Scalar::from(msg_1);

        let ct_div = ElGamal::encrypt_with(&public, msg_0 / msg_1, &(open / scalar));

        assert_eq!(ct_div, ct / scalar);
    }

    #[test]
    fn test_serde_ciphertext() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();
        let msg: u64 = 77;
        let ct = public.encrypt(msg);

        let encoded = bincode::serialize(&ct).unwrap();
        let decoded: ElGamalCiphertext = bincode::deserialize(&encoded).unwrap();

        assert_eq!(ct, decoded);
    }

    #[test]
    fn test_serde_pubkey() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::default();

        let encoded = bincode::serialize(&public).unwrap();
        let decoded: ElGamalPubkey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(public, decoded);
    }

    #[test]
    fn test_serde_secretkey() {
        let ElGamalKeypair { public: _, secret } = ElGamalKeypair::default();

        let encoded = bincode::serialize(&secret).unwrap();
        let decoded: ElGamalSecretKey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(secret, decoded);
    }

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = ElGamalKeypair::default();
        format!("{}/tmp/{}-{}", out_dir, name, keypair.public)
    }

    #[test]
    fn test_write_keypair_file() {
        let outfile = tmp_file_path("test_write_keypair_file.json");
        let serialized_keypair = ElGamalKeypair::default().write_json_file(&outfile).unwrap();
        let keypair_vec: Vec<u8> = serde_json::from_str(&serialized_keypair).unwrap();
        assert!(Path::new(&outfile).exists());
        assert_eq!(
            keypair_vec,
            ElGamalKeypair::read_json_file(&outfile)
                .unwrap()
                .to_bytes()
                .to_vec()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                File::open(&outfile)
                    .expect("open")
                    .metadata()
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_write_keypair_file_overwrite_ok() {
        let outfile = tmp_file_path("test_write_keypair_file_overwrite_ok.json");

        ElGamalKeypair::default().write_json_file(&outfile).unwrap();
        ElGamalKeypair::default().write_json_file(&outfile).unwrap();
    }

    #[test]
    fn test_write_keypair_file_truncate() {
        let outfile = tmp_file_path("test_write_keypair_file_truncate.json");

        ElGamalKeypair::default().write_json_file(&outfile).unwrap();
        ElGamalKeypair::read_json_file(&outfile).unwrap();

        // Ensure outfile is truncated
        {
            let mut f = File::create(&outfile).unwrap();
            f.write_all(String::from_utf8([b'a'; 2048].to_vec()).unwrap().as_bytes())
                .unwrap();
        }
        ElGamalKeypair::default().write_json_file(&outfile).unwrap();
        ElGamalKeypair::read_json_file(&outfile).unwrap();
    }

    #[test]
    fn test_secret_key_new() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        assert_ne!(
            ElGamalSecretKey::new(&keypair1, &Pubkey::default())
                .unwrap()
                .0,
            ElGamalSecretKey::new(&keypair2, &Pubkey::default())
                .unwrap()
                .0,
        );

        let null_signer = NullSigner::new(&Pubkey::default());
        assert!(ElGamalSecretKey::new(&null_signer, &Pubkey::default()).is_err());
    }

    #[test]
    fn test_decrypt_handle_bytes() {
        let handle = DecryptHandle(RistrettoPoint::default());

        let encoded = handle.to_bytes();
        let decoded = DecryptHandle::from_bytes(&encoded).unwrap();

        assert_eq!(handle, decoded);
    }

    #[test]
    fn test_serde_decrypt_handle() {
        let handle = DecryptHandle(RistrettoPoint::default());

        let encoded = bincode::serialize(&handle).unwrap();
        let decoded: DecryptHandle = bincode::deserialize(&encoded).unwrap();

        assert_eq!(handle, decoded);
    }
}
