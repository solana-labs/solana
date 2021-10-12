use {
    crate::encryption::{
        discrete_log::DiscreteLog,
        pedersen::{
            Pedersen, PedersenBase, PedersenCommitment, PedersenDecryptHandle, PedersenOpening,
        },
    },
    arrayref::{array_ref, array_refs},
    core::ops::{Add, Div, Mul, Sub},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
    },
    ed25519_dalek::SecretKey as SigningKey,
    serde::{Deserialize, Serialize},
    solana_sdk::pubkey::Pubkey,
    std::collections::HashMap,
    std::convert::TryInto,
    subtle::{Choice, ConstantTimeEq},
    zeroize::Zeroize,
};
#[cfg(not(target_arch = "bpf"))]
use {
    rand::{rngs::OsRng, CryptoRng, RngCore},
    sha3::Sha3_512,
    std::{
        fmt,
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::Path,
    },
};

struct ElGamal;
impl ElGamal {
    /// On input a randomness generator, the function generates the public and
    /// secret keys for ElGamal encryption.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    fn keygen<T: RngCore + CryptoRng>(rng: &mut T) -> ElGamalKeypair {
        // sample a non-zero scalar
        let mut s: Scalar;
        loop {
            s = Scalar::random(rng);

            if s != Scalar::zero() {
                break;
            }
        }

        let H = PedersenBase::default().H;
        let P = s.invert() * H;

        ElGamalKeypair {
            public: ElGamalPubkey(P),
            secret: ElGamalSecretKey(s),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    pub fn from_signing_key(signing_key: &SigningKey, label: &'static [u8]) -> Self {
        let secret = ElGamalSecretKey::new(signing_key, label);
        let public = ElGamalPubkey::new(&secret);
        Self { secret, public }
    }

    /// On input a public key and a message to be encrypted, the function
    /// returns an ElGamal ciphertext of the message under the public key.
    #[cfg(not(target_arch = "bpf"))]
    fn encrypt<T: Into<Scalar>>(public: &ElGamalPubkey, amount: T) -> ElGamalCiphertext {
        let (message_comm, open) = Pedersen::new(amount);
        let decrypt_handle = public.decrypt_handle(&open);

        ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        }
    }

    /// On input a public key, message, and Pedersen opening, the function
    /// returns an ElGamal ciphertext of the message under the public key using
    /// the opening.
    fn encrypt_with<T: Into<Scalar>>(
        public: &ElGamalPubkey,
        amount: T,
        open: &PedersenOpening,
    ) -> ElGamalCiphertext {
        let message_comm = Pedersen::with(amount, open);
        let decrypt_handle = public.decrypt_handle(open);

        ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        }
    }

    /// On input a secret key and a ciphertext, the function decrypts the ciphertext.
    ///
    /// The output of the function is of type `DiscreteLog`. The exact message
    /// can be recovered via the DiscreteLog's decode method.
    fn decrypt(secret: &ElGamalSecretKey, ct: &ElGamalCiphertext) -> DiscreteLog {
        let ElGamalSecretKey(s) = secret;
        let ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        } = ct;

        DiscreteLog {
            generator: PedersenBase::default().G,
            target: message_comm.get_point() - s * decrypt_handle.get_point(),
        }
    }

    /// On input a secret key and a ciphertext, the function decrypts the
    /// ciphertext for a u32 value.
    fn decrypt_u32(secret: &ElGamalSecretKey, ct: &ElGamalCiphertext) -> Option<u32> {
        let discrete_log_instance = Self::decrypt(secret, ct);
        discrete_log_instance.decode_u32()
    }

    /// On input a secret key, ciphertext, and hashmap, the function decrypts the
    /// ciphertext for a u32 value.
    fn decrypt_u32_online(
        secret: &ElGamalSecretKey,
        ct: &ElGamalCiphertext,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        let discrete_log_instance = Self::decrypt(secret, ct);
        discrete_log_instance.decode_u32_online(hashmap)
    }
}

/// A (twisted) ElGamal encryption keypair.
pub struct ElGamalKeypair {
    /// The public half of this keypair.
    pub public: ElGamalPubkey,
    /// The secret half of this keypair.
    pub secret: ElGamalSecretKey,
}

impl ElGamalKeypair {
    /// Generates the public and secret keys for ElGamal encryption from Ed25519 signing key and an
    /// address.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    pub fn new(signing_key: &SigningKey, address: &Pubkey) -> Self {
        let secret = ElGamalSecretKey::new(signing_key, address);
        let public = ElGamalPubkey::new(&secret);

        Self { secret, public }
    }

    /// Generates the public and secret keys for ElGamal encryption.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(clippy::new_ret_no_self)]
    pub fn default() -> Self {
        Self::with(&mut OsRng) // using OsRng for now
    }

    /// On input a randomness generator, the function generates the public and
    /// secret keys for ElGamal encryption.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    pub fn with<T: RngCore + CryptoRng>(rng: &mut T) -> Self {
        ElGamal::keygen(rng)
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = self.public.to_bytes().to_vec();
        bytes.extend(self.secret.to_bytes());
        bytes.try_into().expect("incorrect length")
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
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElGamalPubkey(RistrettoPoint);
impl ElGamalPubkey {
    /// Derive the `ElGamalPubkey` that uniquely corresponds to an `ElGamalSecretKey`
    #[allow(non_snake_case)]
    pub fn new(secret: &ElGamalSecretKey) -> Self {
        let H = PedersenBase::default().H;
        ElGamalPubkey(secret.0 * H)
    }

    pub fn get_point(&self) -> RistrettoPoint {
        self.0
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

    /// Utility method for code ergonomics.
    #[cfg(not(target_arch = "bpf"))]
    pub fn encrypt<T: Into<Scalar>>(&self, msg: T) -> ElGamalCiphertext {
        ElGamal::encrypt(self, msg)
    }

    /// Utility method for code ergonomics.
    pub fn encrypt_with<T: Into<Scalar>>(
        &self,
        msg: T,
        open: &PedersenOpening,
    ) -> ElGamalCiphertext {
        ElGamal::encrypt_with(self, msg, open)
    }

    /// Generate a decryption token from an ElGamal public key and a Pedersen
    /// opening.
    pub fn decrypt_handle(self, open: &PedersenOpening) -> PedersenDecryptHandle {
        PedersenDecryptHandle::new(&self, open)
    }
}

impl From<RistrettoPoint> for ElGamalPubkey {
    fn from(point: RistrettoPoint) -> ElGamalPubkey {
        ElGamalPubkey(point)
    }
}

impl fmt::Display for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", base64::encode(self.to_bytes()))
    }
}

/// Secret key for the ElGamal encryption scheme.
#[derive(Serialize, Deserialize, Debug, Zeroize)]
#[zeroize(drop)]
pub struct ElGamalSecretKey(Scalar);
impl ElGamalSecretKey {
    pub fn new(signing_key: &SigningKey, address: &Pubkey) -> Self {
        let mut hashable = [0_u8; 64];
        hashable[..32].copy_from_slice(&signing_key.to_bytes());
        hashable[32..].copy_from_slice(&address.to_bytes());
        ElGamalSecretKey(Scalar::hash_from_bytes::<Sha3_512>(&hashable))
    }

    pub fn get_scalar(&self) -> Scalar {
        self.0
    }

    /// Utility method for code ergonomics.
    pub fn decrypt(&self, ct: &ElGamalCiphertext) -> DiscreteLog {
        ElGamal::decrypt(self, ct)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32(&self, ct: &ElGamalCiphertext) -> Option<u32> {
        ElGamal::decrypt_u32(self, ct)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32_online(
        &self,
        ct: &ElGamalCiphertext,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(self, ct, hashmap)
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
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElGamalCiphertext {
    pub message_comm: PedersenCommitment,
    pub decrypt_handle: PedersenDecryptHandle,
}
impl ElGamalCiphertext {
    pub fn add_to_msg<T: Into<Scalar>>(&self, message: T) -> Self {
        let diff_comm = Pedersen::with(message, &PedersenOpening::default());
        ElGamalCiphertext {
            message_comm: self.message_comm + diff_comm,
            decrypt_handle: self.decrypt_handle,
        }
    }

    pub fn sub_to_msg<T: Into<Scalar>>(&self, message: T) -> Self {
        let diff_comm = Pedersen::with(message, &PedersenOpening::default());
        ElGamalCiphertext {
            message_comm: self.message_comm - diff_comm,
            decrypt_handle: self.decrypt_handle,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];

        bytes[..32].copy_from_slice(self.message_comm.get_point().compress().as_bytes());
        bytes[32..].copy_from_slice(self.decrypt_handle.get_point().compress().as_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalCiphertext> {
        let bytes = array_ref![bytes, 0, 64];
        let (message_comm, decrypt_handle) = array_refs![bytes, 32, 32];

        let message_comm = CompressedRistretto::from_slice(message_comm).decompress()?;
        let decrypt_handle = CompressedRistretto::from_slice(decrypt_handle).decompress()?;

        Some(ElGamalCiphertext {
            message_comm: PedersenCommitment(message_comm),
            decrypt_handle: PedersenDecryptHandle(decrypt_handle),
        })
    }

    /// Utility method for code ergonomics.
    pub fn decrypt(&self, secret: &ElGamalSecretKey) -> DiscreteLog {
        ElGamal::decrypt(secret, self)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32(&self, secret: &ElGamalSecretKey) -> Option<u32> {
        ElGamal::decrypt_u32(secret, self)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32_online(
        &self,
        secret: &ElGamalSecretKey,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(secret, self, hashmap)
    }
}

impl From<(PedersenCommitment, PedersenDecryptHandle)> for ElGamalCiphertext {
    fn from((comm, handle): (PedersenCommitment, PedersenDecryptHandle)) -> Self {
        ElGamalCiphertext {
            message_comm: comm,
            decrypt_handle: handle,
        }
    }
}

impl<'a, 'b> Add<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn add(self, other: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm + other.message_comm,
            decrypt_handle: self.decrypt_handle + other.decrypt_handle,
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
            message_comm: self.message_comm - other.message_comm,
            decrypt_handle: self.decrypt_handle - other.decrypt_handle,
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
            message_comm: self.message_comm * other,
            decrypt_handle: self.decrypt_handle * other,
        }
    }
}

define_mul_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Div<&'b Scalar> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn div(self, other: &'b Scalar) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm * other.invert(),
            decrypt_handle: self.decrypt_handle * other.invert(),
        }
    }
}

define_div_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::pedersen::Pedersen;

    #[test]
    fn test_encrypt_decrypt_correctness() {
        let ElGamalKeypair { public, secret } = ElGamalKeypair::default();
        let msg: u32 = 57;
        let ct = ElGamal::encrypt(&public, msg);

        let expected_instance = DiscreteLog {
            generator: PedersenBase::default().G,
            target: Scalar::from(msg) * PedersenBase::default().G,
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
        } = ElGamalKeypair::default();
        let ElGamalKeypair {
            public: pk_2,
            secret: sk_2,
        } = ElGamalKeypair::default();

        let msg: u32 = 77;
        let (comm, open) = Pedersen::new(msg);

        let decrypt_handle_1 = pk_1.decrypt_handle(&open);
        let decrypt_handle_2 = pk_2.decrypt_handle(&open);

        let ct_1: ElGamalCiphertext = (comm, decrypt_handle_1).into();
        let ct_2: ElGamalCiphertext = (comm, decrypt_handle_2).into();

        let expected_instance = DiscreteLog {
            generator: PedersenBase::default().G,
            target: Scalar::from(msg) * PedersenBase::default().G,
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
}
