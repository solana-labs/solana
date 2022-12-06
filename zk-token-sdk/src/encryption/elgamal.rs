//! The twisted ElGamal encryption implementation.
//!
//! The message space consists of any number that is representable as a scalar (a.k.a. "exponent")
//! for Curve25519.
//!
//! A twisted ElGamal ciphertext consists of two components:
//! - A Pedersen commitment that encodes a message to be encrypted
//! - A "decryption handle" that binds the Pedersen opening to a specific public key
//! In contrast to the traditional ElGamal encryption scheme, the twisted ElGamal encodes messages
//! directly as a Pedersen commitment. Therefore, proof systems that are designed specifically for
//! Pedersen commitments can be used on the twisted ElGamal ciphertexts.
//!
//! As the messages are encrypted as scalar elements (a.k.a. in the "exponent"), one must solve the
//! discrete log to recover the originally encrypted value.

use {
    crate::encryption::{
        discrete_log::DiscreteLog,
        pedersen::{Pedersen, PedersenCommitment, PedersenOpening, G, H},
    },
    core::ops::{Add, Mul, Sub},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::Identity,
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
#[cfg(not(target_os = "solana"))]
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
pub struct ElGamal;
impl ElGamal {
    /// Generates an ElGamal keypair.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_os = "solana"))]
    #[allow(non_snake_case)]
    fn keygen() -> ElGamalKeypair {
        // secret scalar should be non-zero except with negligible probability
        let mut s = Scalar::random(&mut OsRng);
        let keypair = Self::keygen_with_scalar(&s);

        s.zeroize();
        keypair
    }

    /// Generates an ElGamal keypair from a scalar input that determines the ElGamal private key.
    ///
    /// This function panics if the input scalar is zero, which is not a valid key.
    #[cfg(not(target_os = "solana"))]
    #[allow(non_snake_case)]
    fn keygen_with_scalar(s: &Scalar) -> ElGamalKeypair {
        let secret = ElGamalSecretKey(*s);
        let public = ElGamalPubkey::new(&secret);

        ElGamalKeypair { public, secret }
    }

    /// On input an ElGamal public key and an amount to be encrypted, the function returns a
    /// corresponding ElGamal ciphertext.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_os = "solana"))]
    fn encrypt<T: Into<Scalar>>(public: &ElGamalPubkey, amount: T) -> ElGamalCiphertext {
        let (commitment, opening) = Pedersen::new(amount);
        let handle = public.decrypt_handle(&opening);

        ElGamalCiphertext { commitment, handle }
    }

    /// On input a public key, amount, and Pedersen opening, the function returns the corresponding
    /// ElGamal ciphertext.
    #[cfg(not(target_os = "solana"))]
    fn encrypt_with<T: Into<Scalar>>(
        amount: T,
        public: &ElGamalPubkey,
        opening: &PedersenOpening,
    ) -> ElGamalCiphertext {
        let commitment = Pedersen::with(amount, opening);
        let handle = public.decrypt_handle(opening);

        ElGamalCiphertext { commitment, handle }
    }

    /// On input an amount, the function returns a twisted ElGamal ciphertext where the associated
    /// Pedersen opening is always zero. Since the opening is zero, any twisted ElGamal ciphertext
    /// of this form is a valid ciphertext under any ElGamal public key.
    #[cfg(not(target_os = "solana"))]
    pub fn encode<T: Into<Scalar>>(amount: T) -> ElGamalCiphertext {
        let commitment = Pedersen::encode(amount);
        let handle = DecryptHandle(RistrettoPoint::identity());

        ElGamalCiphertext { commitment, handle }
    }

    /// On input a secret key and a ciphertext, the function returns the discrete log encoding of
    /// original amount.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover, the originally encrypted
    /// amount, use `DiscreteLog::decode`.
    #[cfg(not(target_os = "solana"))]
    fn decrypt(secret: &ElGamalSecretKey, ciphertext: &ElGamalCiphertext) -> DiscreteLog {
        DiscreteLog::new(
            *G,
            &ciphertext.commitment.0 - &(&secret.0 * &ciphertext.handle.0),
        )
    }

    /// On input a secret key and a ciphertext, the function returns the decrypted amount
    /// interpretted as a positive 32-bit number (but still of type `u64`).
    ///
    /// If the originally encrypted amount is not a positive 32-bit number, then the function
    /// returns `None`.
    #[cfg(not(target_os = "solana"))]
    fn decrypt_u32(secret: &ElGamalSecretKey, ciphertext: &ElGamalCiphertext) -> Option<u64> {
        let discrete_log_instance = Self::decrypt(secret, ciphertext);
        discrete_log_instance.decode_u32()
    }
}

/// A (twisted) ElGamal encryption keypair.
///
/// The instances of the secret key are zeroized on drop.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, Zeroize)]
pub struct ElGamalKeypair {
    /// The public half of this keypair.
    pub public: ElGamalPubkey,
    /// The secret half of this keypair.
    pub secret: ElGamalSecretKey,
}

impl ElGamalKeypair {
    /// Deterministically derives an ElGamal keypair from an Ed25519 signing key and a Solana
    /// address.
    ///
    /// This function exists for applications where a user may not wish to maintin a Solana
    /// (Ed25519) keypair and an ElGamal keypair separately. A user may wish to solely maintain the
    /// Solana keypair and then derive the ElGamal keypair on-the-fly whenever
    /// encryption/decryption is needed.
    ///
    /// For the spl token-2022 confidential extension application, the ElGamal encryption public
    /// key is specified in a token account address. A natural way to derive an ElGamal keypair is
    /// then to define it from the hash of a Solana keypair and a Solana address. However, for
    /// general hardware wallets, the signing key is not exposed in the API. Therefore, this
    /// function uses a signer to sign a pre-specified message with respect to a Solana address.
    /// The resulting signature is then hashed to derive an ElGamal keypair.
    #[cfg(not(target_os = "solana"))]
    #[allow(non_snake_case)]
    pub fn new(signer: &dyn Signer, address: &Pubkey) -> Result<Self, SignerError> {
        let secret = ElGamalSecretKey::new(signer, address)?;
        let public = ElGamalPubkey::new(&secret);
        Ok(ElGamalKeypair { public, secret })
    }

    /// Generates the public and secret keys for ElGamal encryption.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_os = "solana"))]
    #[allow(clippy::new_ret_no_self)]
    pub fn new_rand() -> Self {
        ElGamal::keygen()
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&self.public.to_bytes());
        bytes[32..].copy_from_slice(self.secret.as_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 64 {
            return None;
        }

        Some(Self {
            public: ElGamalPubkey::from_bytes(&bytes[..32])?,
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
        let s = &secret.0;
        assert!(s != &Scalar::zero());

        ElGamalPubkey(s.invert() * &(*H))
    }

    pub fn get_point(&self) -> &RistrettoPoint {
        &self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalPubkey> {
        if bytes.len() != 32 {
            return None;
        }

        Some(ElGamalPubkey(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }

    /// Encrypts an amount under the public key.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    #[cfg(not(target_os = "solana"))]
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
    ///
    /// See `ElGamalKeypair::new` for more context on the key derivation.
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
        if bool::from(signature.as_ref().ct_eq(Signature::default().as_ref())) {
            return Err(SignerError::Custom("Rejecting default signatures".into()));
        }

        Ok(ElGamalSecretKey(Scalar::hash_from_bytes::<Sha3_512>(
            signature.as_ref(),
        )))
    }

    /// Randomly samples an ElGamal secret key.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    pub fn new_rand() -> Self {
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
    pub fn decrypt_u32(&self, ciphertext: &ElGamalCiphertext) -> Option<u64> {
        ElGamal::decrypt_u32(self, ciphertext)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalSecretKey> {
        match bytes.try_into() {
            Ok(bytes) => Scalar::from_canonical_bytes(bytes).map(ElGamalSecretKey),
            _ => None,
        }
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
    pub fn add_amount<T: Into<Scalar>>(&self, amount: T) -> Self {
        let commitment_to_add = PedersenCommitment(amount.into() * &(*G));
        ElGamalCiphertext {
            commitment: &self.commitment + &commitment_to_add,
            handle: self.handle,
        }
    }

    pub fn subtract_amount<T: Into<Scalar>>(&self, amount: T) -> Self {
        let commitment_to_subtract = PedersenCommitment(amount.into() * &(*G));
        ElGamalCiphertext {
            commitment: &self.commitment - &commitment_to_subtract,
            handle: self.handle,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&self.commitment.to_bytes());
        bytes[32..].copy_from_slice(&self.handle.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalCiphertext> {
        if bytes.len() != 64 {
            return None;
        }

        Some(ElGamalCiphertext {
            commitment: PedersenCommitment::from_bytes(&bytes[..32])?,
            handle: DecryptHandle::from_bytes(&bytes[32..])?,
        })
    }

    /// Decrypts the ciphertext using an ElGamal secret key.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover, the originally encrypted
    /// amount, use `DiscreteLog::decode`.
    pub fn decrypt(&self, secret: &ElGamalSecretKey) -> DiscreteLog {
        ElGamal::decrypt(secret, self)
    }

    /// Decrypts the ciphertext using an ElGamal secret key assuming that the message is a positive
    /// 32-bit number.
    ///
    /// If the originally encrypted amount is not a positive 32-bit number, then the function
    /// returns `None`.
    pub fn decrypt_u32(&self, secret: &ElGamalSecretKey) -> Option<u64> {
        ElGamal::decrypt_u32(secret, self)
    }
}

impl fmt::Display for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", base64::encode(self.to_bytes()))
    }
}

impl<'a, 'b> Add<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn add(self, ciphertext: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment + &ciphertext.commitment,
            handle: &self.handle + &ciphertext.handle,
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

    fn sub(self, ciphertext: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment - &ciphertext.commitment,
            handle: &self.handle - &ciphertext.handle,
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

    fn mul(self, scalar: &'b Scalar) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: &self.commitment * scalar,
            handle: &self.handle * scalar,
        }
    }
}

define_mul_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Mul<&'b ElGamalCiphertext> for &'a Scalar {
    type Output = ElGamalCiphertext;

    fn mul(self, ciphertext: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            commitment: self * &ciphertext.commitment,
            handle: self * &ciphertext.handle,
        }
    }
}

define_mul_variants!(
    LHS = Scalar,
    RHS = ElGamalCiphertext,
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
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<DecryptHandle> {
        if bytes.len() != 32 {
            return None;
        }

        Some(DecryptHandle(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }
}

impl<'a, 'b> Add<&'b DecryptHandle> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn add(self, handle: &'b DecryptHandle) -> DecryptHandle {
        DecryptHandle(&self.0 + &handle.0)
    }
}

define_add_variants!(
    LHS = DecryptHandle,
    RHS = DecryptHandle,
    Output = DecryptHandle
);

impl<'a, 'b> Sub<&'b DecryptHandle> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn sub(self, handle: &'b DecryptHandle) -> DecryptHandle {
        DecryptHandle(&self.0 - &handle.0)
    }
}

define_sub_variants!(
    LHS = DecryptHandle,
    RHS = DecryptHandle,
    Output = DecryptHandle
);

impl<'a, 'b> Mul<&'b Scalar> for &'a DecryptHandle {
    type Output = DecryptHandle;

    fn mul(self, scalar: &'b Scalar) -> DecryptHandle {
        DecryptHandle(&self.0 * scalar)
    }
}

define_mul_variants!(LHS = DecryptHandle, RHS = Scalar, Output = DecryptHandle);

impl<'a, 'b> Mul<&'b DecryptHandle> for &'a Scalar {
    type Output = DecryptHandle;

    fn mul(self, handle: &'b DecryptHandle) -> DecryptHandle {
        DecryptHandle(self * &handle.0)
    }
}

define_mul_variants!(LHS = Scalar, RHS = DecryptHandle, Output = DecryptHandle);

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::encryption::pedersen::Pedersen,
        solana_sdk::{signature::Keypair, signer::null_signer::NullSigner},
    };

    #[test]
    fn test_encrypt_decrypt_correctness() {
        let ElGamalKeypair { public, secret } = ElGamalKeypair::new_rand();
        let amount: u32 = 57;
        let ciphertext = ElGamal::encrypt(&public, amount);

        let expected_instance = DiscreteLog::new(*G, Scalar::from(amount) * &(*G));

        assert_eq!(expected_instance, ElGamal::decrypt(&secret, &ciphertext));
        assert_eq!(57_u64, secret.decrypt_u32(&ciphertext).unwrap());
    }

    #[test]
    fn test_encrypt_decrypt_correctness_multithreaded() {
        let ElGamalKeypair { public, secret } = ElGamalKeypair::new_rand();
        let amount: u32 = 57;
        let ciphertext = ElGamal::encrypt(&public, amount);

        let mut instance = ElGamal::decrypt(&secret, &ciphertext);
        instance.num_threads(4).unwrap();
        assert_eq!(57_u64, instance.decode_u32().unwrap());
    }

    #[test]
    fn test_decrypt_handle() {
        let ElGamalKeypair {
            public: public_0,
            secret: secret_0,
        } = ElGamalKeypair::new_rand();
        let ElGamalKeypair {
            public: public_1,
            secret: secret_1,
        } = ElGamalKeypair::new_rand();

        let amount: u32 = 77;
        let (commitment, opening) = Pedersen::new(amount);

        let handle_0 = public_0.decrypt_handle(&opening);
        let handle_1 = public_1.decrypt_handle(&opening);

        let ciphertext_0 = ElGamalCiphertext {
            commitment,
            handle: handle_0,
        };
        let ciphertext_1 = ElGamalCiphertext {
            commitment,
            handle: handle_1,
        };

        let expected_instance = DiscreteLog::new(*G, Scalar::from(amount) * &(*G));

        assert_eq!(expected_instance, secret_0.decrypt(&ciphertext_0));
        assert_eq!(expected_instance, secret_1.decrypt(&ciphertext_1));
    }

    #[test]
    fn test_homomorphic_addition() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::new_rand();
        let amount_0: u64 = 57;
        let amount_1: u64 = 77;

        // Add two ElGamal ciphertexts
        let opening_0 = PedersenOpening::new_rand();
        let opening_1 = PedersenOpening::new_rand();

        let ciphertext_0 = ElGamal::encrypt_with(amount_0, &public, &opening_0);
        let ciphertext_1 = ElGamal::encrypt_with(amount_1, &public, &opening_1);

        let ciphertext_sum =
            ElGamal::encrypt_with(amount_0 + amount_1, &public, &(&opening_0 + &opening_1));

        assert_eq!(ciphertext_sum, ciphertext_0 + ciphertext_1);

        // Add to ElGamal ciphertext
        let opening = PedersenOpening::new_rand();
        let ciphertext = ElGamal::encrypt_with(amount_0, &public, &opening);
        let ciphertext_sum = ElGamal::encrypt_with(amount_0 + amount_1, &public, &opening);

        assert_eq!(ciphertext_sum, ciphertext.add_amount(amount_1));
    }

    #[test]
    fn test_homomorphic_subtraction() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::new_rand();
        let amount_0: u64 = 77;
        let amount_1: u64 = 55;

        // Subtract two ElGamal ciphertexts
        let opening_0 = PedersenOpening::new_rand();
        let opening_1 = PedersenOpening::new_rand();

        let ciphertext_0 = ElGamal::encrypt_with(amount_0, &public, &opening_0);
        let ciphertext_1 = ElGamal::encrypt_with(amount_1, &public, &opening_1);

        let ciphertext_sub =
            ElGamal::encrypt_with(amount_0 - amount_1, &public, &(&opening_0 - &opening_1));

        assert_eq!(ciphertext_sub, ciphertext_0 - ciphertext_1);

        // Subtract to ElGamal ciphertext
        let opening = PedersenOpening::new_rand();
        let ciphertext = ElGamal::encrypt_with(amount_0, &public, &opening);
        let ciphertext_sub = ElGamal::encrypt_with(amount_0 - amount_1, &public, &opening);

        assert_eq!(ciphertext_sub, ciphertext.subtract_amount(amount_1));
    }

    #[test]
    fn test_homomorphic_multiplication() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::new_rand();
        let amount_0: u64 = 57;
        let amount_1: u64 = 77;

        let opening = PedersenOpening::new_rand();

        let ciphertext = ElGamal::encrypt_with(amount_0, &public, &opening);
        let scalar = Scalar::from(amount_1);

        let ciphertext_prod =
            ElGamal::encrypt_with(amount_0 * amount_1, &public, &(&opening * scalar));

        assert_eq!(ciphertext_prod, ciphertext * scalar);
        assert_eq!(ciphertext_prod, scalar * ciphertext);
    }

    #[test]
    fn test_serde_ciphertext() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::new_rand();
        let amount: u64 = 77;
        let ciphertext = public.encrypt(amount);

        let encoded = bincode::serialize(&ciphertext).unwrap();
        let decoded: ElGamalCiphertext = bincode::deserialize(&encoded).unwrap();

        assert_eq!(ciphertext, decoded);
    }

    #[test]
    fn test_serde_pubkey() {
        let ElGamalKeypair { public, secret: _ } = ElGamalKeypair::new_rand();

        let encoded = bincode::serialize(&public).unwrap();
        let decoded: ElGamalPubkey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(public, decoded);
    }

    #[test]
    fn test_serde_secretkey() {
        let ElGamalKeypair { public: _, secret } = ElGamalKeypair::new_rand();

        let encoded = bincode::serialize(&secret).unwrap();
        let decoded: ElGamalSecretKey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(secret, decoded);
    }

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = ElGamalKeypair::new_rand();
        format!("{}/tmp/{}-{}", out_dir, name, keypair.public)
    }

    #[test]
    fn test_write_keypair_file() {
        let outfile = tmp_file_path("test_write_keypair_file.json");
        let serialized_keypair = ElGamalKeypair::new_rand()
            .write_json_file(&outfile)
            .unwrap();
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

        ElGamalKeypair::new_rand()
            .write_json_file(&outfile)
            .unwrap();
        ElGamalKeypair::new_rand()
            .write_json_file(&outfile)
            .unwrap();
    }

    #[test]
    fn test_write_keypair_file_truncate() {
        let outfile = tmp_file_path("test_write_keypair_file_truncate.json");

        ElGamalKeypair::new_rand()
            .write_json_file(&outfile)
            .unwrap();
        ElGamalKeypair::read_json_file(&outfile).unwrap();

        // Ensure outfile is truncated
        {
            let mut f = File::create(&outfile).unwrap();
            f.write_all(String::from_utf8([b'a'; 2048].to_vec()).unwrap().as_bytes())
                .unwrap();
        }
        ElGamalKeypair::new_rand()
            .write_json_file(&outfile)
            .unwrap();
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
