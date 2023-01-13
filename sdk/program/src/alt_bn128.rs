pub mod prelude {
    pub use crate::alt_bn128::{consts::*, target_arch::*, AltBn128Error};
}

use {
    bytemuck::{Pod, Zeroable},
    consts::*,
    thiserror::Error,
};

mod consts {
    /// Input length for the add operation.
    pub const ALT_BN128_ADDITION_INPUT_LEN: usize = 128;

    /// Input length for the multiplication operation.
    pub const ALT_BN128_MULTIPLICATION_INPUT_LEN: usize = 128;

    /// Pair element length.
    pub const ALT_BN128_PAIRING_ELEMENT_LEN: usize = 192;

    /// Output length for the add operation.
    pub const ALT_BN128_ADDITION_OUTPUT_LEN: usize = 64;

    /// Output length for the multiplication operation.
    pub const ALT_BN128_MULTIPLICATION_OUTPUT_LEN: usize = 64;

    /// Output length for pairing operation..
    pub const ALT_BN128_PAIRING_OUTPUT_LEN: usize = 32;

    /// Size of the EC point field, in bytes.
    pub const ALT_BN128_FIELD_SIZE: usize = 32;

    /// Size of the EC point. `alt_bn128` point contains
    /// the consistently united x and y fields as 64 bytes.
    pub const ALT_BN128_POINT_SIZE: usize = 64;

    pub const ALT_BN128_ADD: u64 = 0;
    pub const ALT_BN128_SUB: u64 = 1;
    pub const ALT_BN128_MUL: u64 = 2;
    pub const ALT_BN128_PAIRING: u64 = 3;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AltBn128Error {
    #[error("The input data is invalid")]
    InvalidInputData,
    #[error("Invalid group data")]
    GroupError,
    #[error("Slice data is going out of input data bounds")]
    SliceOutOfBounds,
    #[error("Unexpected error")]
    UnexpectedError,
    #[error("Failed to convert a byte slice into a vector {0:?}")]
    TryIntoVecError(Vec<u8>),
}

impl From<u64> for AltBn128Error {
    fn from(v: u64) -> AltBn128Error {
        match v {
            1 => AltBn128Error::InvalidInputData,
            2 => AltBn128Error::GroupError,
            3 => AltBn128Error::SliceOutOfBounds,
            4 => AltBn128Error::TryIntoVecError(Vec::new()),
            _ => AltBn128Error::UnexpectedError,
        }
    }
}

impl From<AltBn128Error> for u64 {
    fn from(v: AltBn128Error) -> u64 {
        match v {
            AltBn128Error::InvalidInputData => 1,
            AltBn128Error::GroupError => 2,
            AltBn128Error::SliceOutOfBounds => 3,
            AltBn128Error::TryIntoVecError(_) => 4,
            AltBn128Error::UnexpectedError => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodG1(pub [u8; 64]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodG2(pub [u8; 128]);

#[cfg(not(target_os = "solana"))]
mod target_arch {
    use {
        super::*,
        ark_bn254::{self, Parameters},
        ark_ec::{
            self,
            models::bn::{g1::G1Prepared, g2::G2Prepared, Bn},
            AffineCurve, PairingEngine, ProjectiveCurve,
        },
        ark_ff::{
            bytes::{FromBytes, ToBytes},
            BigInteger, BigInteger256, One, Zero,
        },
    };

    type G1 = ark_ec::short_weierstrass_jacobian::GroupAffine<ark_bn254::g1::Parameters>;
    type G2 = ark_ec::short_weierstrass_jacobian::GroupAffine<ark_bn254::g2::Parameters>;

    impl TryFrom<PodG1> for G1 {
        type Error = AltBn128Error;

        fn try_from(bytes: PodG1) -> Result<Self, Self::Error> {
            if bytes.0 == [0u8; 64] {
                return Ok(G1::zero());
            }
            let g1 = <Self as FromBytes>::read(&*[&bytes.0[..], &[0u8][..]].concat());

            match g1 {
                Ok(g1) => {
                    if !g1.is_on_curve() {
                        Err(AltBn128Error::GroupError)
                    } else {
                        Ok(g1)
                    }
                }
                Err(_) => Err(AltBn128Error::InvalidInputData),
            }
        }
    }

    impl TryFrom<PodG2> for G2 {
        type Error = AltBn128Error;

        fn try_from(bytes: PodG2) -> Result<Self, Self::Error> {
            if bytes.0 == [0u8; 128] {
                return Ok(G2::zero());
            }
            let g2 = <Self as FromBytes>::read(&*[&bytes.0[..], &[0u8][..]].concat());

            match g2 {
                Ok(g2) => {
                    if !g2.is_on_curve() {
                        Err(AltBn128Error::GroupError)
                    } else {
                        Ok(g2)
                    }
                }
                Err(_) => Err(AltBn128Error::InvalidInputData),
            }
        }
    }

    pub fn alt_bn128_addition(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input.len() > ALT_BN128_ADDITION_INPUT_LEN {
            return Err(AltBn128Error::InvalidInputData);
        }

        let mut input = input.to_vec();
        input.resize(ALT_BN128_ADDITION_INPUT_LEN, 0);

        let p: G1 = PodG1(
            convert_edianness_64(&input[..64])
                .try_into()
                .map_err(AltBn128Error::TryIntoVecError)?,
        )
        .try_into()?;
        let q: G1 = PodG1(
            convert_edianness_64(&input[64..ALT_BN128_ADDITION_INPUT_LEN])
                .try_into()
                .map_err(AltBn128Error::TryIntoVecError)?,
        )
        .try_into()?;

        let mut result_point_data = [0; ALT_BN128_ADDITION_OUTPUT_LEN + 1];
        let result_point = p + q;
        <G1 as ToBytes>::write(&result_point, &mut result_point_data[..])
            .map_err(|_| AltBn128Error::InvalidInputData)?;

        if result_point == G1::zero() {
            return Ok([0u8; ALT_BN128_ADDITION_OUTPUT_LEN].to_vec());
        }
        Ok(convert_edianness_64(&result_point_data[..ALT_BN128_ADDITION_OUTPUT_LEN]).to_vec())
    }

    pub fn alt_bn128_multiplication(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input.len() > ALT_BN128_MULTIPLICATION_INPUT_LEN {
            return Err(AltBn128Error::InvalidInputData);
        }

        let mut input = input.to_vec();
        input.resize(ALT_BN128_MULTIPLICATION_INPUT_LEN, 0);

        let p: G1 = PodG1(
            convert_edianness_64(&input[..64])
                .try_into()
                .map_err(AltBn128Error::TryIntoVecError)?,
        )
        .try_into()?;
        let fr = <BigInteger256 as FromBytes>::read(convert_edianness_64(&input[64..96]).as_ref())
            .map_err(|_| AltBn128Error::InvalidInputData)?;

        let mut result_point_data = [0; ALT_BN128_MULTIPLICATION_OUTPUT_LEN + 1];
        let result_point: G1 = p.into_projective().mul(&fr).into();
        <G1 as ToBytes>::write(&result_point, &mut result_point_data[..])
            .map_err(|_| AltBn128Error::InvalidInputData)?;
        if result_point == G1::zero() {
            return Ok([0u8; ALT_BN128_MULTIPLICATION_OUTPUT_LEN].to_vec());
        }
        Ok(
            convert_edianness_64(&result_point_data[..ALT_BN128_MULTIPLICATION_OUTPUT_LEN])
                .to_vec(),
        )
    }

    pub fn alt_bn128_pairing(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input
            .len()
            .checked_rem(consts::ALT_BN128_PAIRING_ELEMENT_LEN)
            .is_none()
        {
            return Err(AltBn128Error::InvalidInputData);
        }

        let ele_len = input.len().saturating_div(ALT_BN128_PAIRING_ELEMENT_LEN);
        let mut vec_pairs: Vec<(G1Prepared<Parameters>, G2Prepared<Parameters>)> = Vec::new();
        for i in 0..ele_len {
            let g1: G1 = PodG1(
                convert_edianness_64(
                    &input[i.saturating_mul(ALT_BN128_PAIRING_ELEMENT_LEN)
                        ..i.saturating_mul(ALT_BN128_PAIRING_ELEMENT_LEN)
                            .saturating_add(ALT_BN128_POINT_SIZE)],
                )
                .try_into()
                .map_err(AltBn128Error::TryIntoVecError)?,
            )
            .try_into()?;
            let g2: G2 = PodG2(
                convert_edianness_128(
                    &input[i
                        .saturating_mul(ALT_BN128_PAIRING_ELEMENT_LEN)
                        .saturating_add(ALT_BN128_POINT_SIZE)
                        ..i.saturating_mul(ALT_BN128_PAIRING_ELEMENT_LEN)
                            .saturating_add(ALT_BN128_PAIRING_ELEMENT_LEN)],
                )
                .try_into()
                .map_err(AltBn128Error::TryIntoVecError)?,
            )
            .try_into()?;
            vec_pairs.push((g1.into(), g2.into()));
        }

        let mut result = BigInteger256::from(0u64);
        let res =
            <Bn<Parameters> as PairingEngine>::product_of_pairings((vec_pairs[..ele_len]).iter());
        type GT = <ark_ec::models::bn::Bn<ark_bn254::Parameters> as ark_ec::PairingEngine>::Fqk;
        if res == GT::one() {
            result = BigInteger256::from(1u64);
        }

        let output = result.to_bytes_be();
        Ok(output)
    }

    fn convert_edianness_64(bytes: &[u8]) -> Vec<u8> {
        bytes
            .chunks(32)
            .flat_map(|b| b.iter().copied().rev().collect::<Vec<u8>>())
            .collect::<Vec<u8>>()
    }

    fn convert_edianness_128(bytes: &[u8]) -> Vec<u8> {
        bytes
            .chunks(64)
            .flat_map(|b| b.iter().copied().rev().collect::<Vec<u8>>())
            .collect::<Vec<u8>>()
    }
}

#[cfg(target_os = "solana")]
mod target_arch {
    use super::*;
    pub fn alt_bn128_addition(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input.len() > ALT_BN128_ADDITION_INPUT_LEN {
            return Err(AltBn128Error::InvalidInputData);
        }
        let mut result_buffer = [0; ALT_BN128_ADDITION_OUTPUT_LEN];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_group_op(
                ALT_BN128_ADD,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer.to_vec()),
            error => Err(AltBn128Error::from(error)),
        }
    }

    pub fn alt_bn128_multiplication(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input.len() > ALT_BN128_MULTIPLICATION_INPUT_LEN {
            return Err(AltBn128Error::InvalidInputData);
        }
        let mut result_buffer = [0u8; ALT_BN128_POINT_SIZE];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_group_op(
                ALT_BN128_MUL,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer.to_vec()),
            error => Err(AltBn128Error::from(error)),
        }
    }

    pub fn alt_bn128_pairing(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
        if input
            .len()
            .checked_rem(consts::ALT_BN128_PAIRING_ELEMENT_LEN)
            .is_none()
        {
            return Err(AltBn128Error::InvalidInputData);
        }
        let mut result_buffer = [0u8; 32];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_group_op(
                ALT_BN128_PAIRING,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer.to_vec()),
            error => Err(AltBn128Error::from(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::alt_bn128::prelude::*;

    #[test]
    fn alt_bn128_addition_test() {
        use serde::Deserialize;

        let test_data = r#"[
        {
            "Input": "18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f3726607c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
            "Expected": "2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c915",
            "Name": "chfast1",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "2243525c5efd4b9c3d3c45ac0ca3fe4dd85e830a4ce6b65fa1eeaee202839703301d1d33be6da8e509df21cc35964723180eed7532537db9ae5e7d48f195c91518b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266",
            "Expected": "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb721611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb204",
            "Name": "chfast2",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Name": "cdetrio1",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Name": "cdetrio2",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Name": "cdetrio3",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Name": "cdetrio4",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Name": "cdetrio5",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Name": "cdetrio6",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "0000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "Expected": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Gas": 150,
            "Name": "cdetrio7",
            "NoBenchmark": false
        },{
            "Input": "0000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002",
            "Expected": "030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd315ed738c0e0a7c92e7845f96b2ae9c0a68a6a449e3538fc7ff3ebf7a5a18a2c4",
            "Name": "cdetrio8",
            "Gas": 150,
            "NoBenchmark": false
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d98",
            "Expected": "15bf2bb17880144b5d1cd2b1f46eff9d617bffd1ca57c37fb5a49bd84e53cf66049c797f9ce0d17083deb32b5e36f2ea2a212ee036598dd7624c168993d1355f",
            "Name": "cdetrio9",
            "Gas": 150,
            "NoBenchmark": false
        }
        ]"#;

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct TestCase {
            input: String,
            expected: String,
        }

        let test_cases: Vec<TestCase> = serde_json::from_str(test_data).unwrap();

        test_cases.iter().for_each(|test| {
            let input = array_bytes::hex2bytes_unchecked(&test.input);
            let result = alt_bn128_addition(&input);
            assert!(result.is_ok());
            let result = result.unwrap();

            let expected = array_bytes::hex2bytes_unchecked(&test.expected);
            assert_eq!(result, expected);
        });
    }

    #[test]
    fn alt_bn128_multiplication_test() {
        use serde::Deserialize;

        let test_data = r#"[
        {
            "Input": "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb721611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb20400000000000000000000000000000000000000000000000011138ce750fa15c2",
            "Expected": "070a8d6a982153cae4be29d434e8faef8a47b274a053f5a4ee2a6c9c13c31e5c031b8ce914eba3a9ffb989f9cdd5b0f01943074bf4f0f315690ec3cec6981afc",
            "Name": "chfast1",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "070a8d6a982153cae4be29d434e8faef8a47b274a053f5a4ee2a6c9c13c31e5c031b8ce914eba3a9ffb989f9cdd5b0f01943074bf4f0f315690ec3cec6981afc30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd46",
            "Expected": "025a6f4181d2b4ea8b724290ffb40156eb0adb514c688556eb79cdea0752c2bb2eff3f31dea215f1eb86023a133a996eb6300b44da664d64251d05381bb8a02e",
            "Name": "chfast2",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "025a6f4181d2b4ea8b724290ffb40156eb0adb514c688556eb79cdea0752c2bb2eff3f31dea215f1eb86023a133a996eb6300b44da664d64251d05381bb8a02e183227397098d014dc2822db40c0ac2ecbc0b548b438e5469e10460b6c3e7ea3",
            "Expected": "14789d0d4a730b354403b5fac948113739e276c23e0258d8596ee72f9cd9d3230af18a63153e0ec25ff9f2951dd3fa90ed0197bfef6e2a1a62b5095b9d2b4a27",
            "Name": "chfast3",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f6ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "Expected": "2cde5879ba6f13c0b5aa4ef627f159a3347df9722efce88a9afbb20b763b4c411aa7e43076f6aee272755a7f9b84832e71559ba0d2e0b17d5f9f01755e5b0d11",
            "Name": "cdetrio1",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f630644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000",
            "Expected": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe3163511ddc1c3f25d396745388200081287b3fd1472d8339d5fecb2eae0830451",
            "Name": "cdetrio2",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f60000000000000000000000000000000100000000000000000000000000000000",
            "Expected": "1051acb0700ec6d42a88215852d582efbaef31529b6fcbc3277b5c1b300f5cf0135b2394bb45ab04b8bd7611bd2dfe1de6a4e6e2ccea1ea1955f577cd66af85b",
            "Name": "cdetrio3",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f60000000000000000000000000000000000000000000000000000000000000009",
            "Expected": "1dbad7d39dbc56379f78fac1bca147dc8e66de1b9d183c7b167351bfe0aeab742cd757d51289cd8dbd0acf9e673ad67d0f0a89f912af47ed1be53664f5692575",
            "Name": "cdetrio4",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f60000000000000000000000000000000000000000000000000000000000000001",
            "Expected": "1a87b0584ce92f4593d161480614f2989035225609f08058ccfa3d0f940febe31a2f3c951f6dadcc7ee9007dff81504b0fcd6d7cf59996efdc33d92bf7f9f8f6",
            "Name": "cdetrio5",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7cffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "Expected": "29e587aadd7c06722aabba753017c093f70ba7eb1f1c0104ec0564e7e3e21f6022b1143f6a41008e7755c71c3d00b6b915d386de21783ef590486d8afa8453b1",
            "Name": "cdetrio6",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000",
            "Expected": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa92e83f8d734803fc370eba25ed1f6b8768bd6d83887b87165fc2434fe11a830cb",
            "Name": "cdetrio7",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c0000000000000000000000000000000100000000000000000000000000000000",
            "Expected": "221a3577763877920d0d14a91cd59b9479f83b87a653bb41f82a3f6f120cea7c2752c7f64cdd7f0e494bff7b60419f242210f2026ed2ec70f89f78a4c56a1f15",
            "Name": "cdetrio8",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c0000000000000000000000000000000000000000000000000000000000000009",
            "Expected": "228e687a379ba154554040f8821f4e41ee2be287c201aa9c3bc02c9dd12f1e691e0fd6ee672d04cfd924ed8fdc7ba5f2d06c53c1edc30f65f2af5a5b97f0a76a",
            "Name": "cdetrio9",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c0000000000000000000000000000000000000000000000000000000000000001",
            "Expected": "17c139df0efee0f766bc0204762b774362e4ded88953a39ce849a8a7fa163fa901e0559bacb160664764a357af8a9fe70baa9258e0b959273ffc5718c6d4cc7c",
            "Name": "cdetrio10",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d98ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "Expected": "00a1a234d08efaa2616607e31eca1980128b00b415c845ff25bba3afcb81dc00242077290ed33906aeb8e42fd98c41bcb9057ba03421af3f2d08cfc441186024",
            "Name": "cdetrio11",
            "Gas": 6000,
            "NoBenchmark": false
        },{
            "Input": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d9830644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000",
            "Expected": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b8692929ee761a352600f54921df9bf472e66217e7bb0cee9032e00acc86b3c8bfaf",
            "Name": "cdetrio12",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d980000000000000000000000000000000100000000000000000000000000000000",
            "Expected": "1071b63011e8c222c5a771dfa03c2e11aac9666dd097f2c620852c3951a4376a2f46fe2f73e1cf310a168d56baa5575a8319389d7bfa6b29ee2d908305791434",
            "Name": "cdetrio13",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d980000000000000000000000000000000000000000000000000000000000000009",
            "Expected": "19f75b9dd68c080a688774a6213f131e3052bd353a304a189d7a2ee367e3c2582612f545fb9fc89fde80fd81c68fc7dcb27fea5fc124eeda69433cf5c46d2d7f",
            "Name": "cdetrio14",
            "Gas": 6000,
            "NoBenchmark": true
        },{
            "Input": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d980000000000000000000000000000000000000000000000000000000000000001",
            "Expected": "039730ea8dff1254c0fee9c0ea777d29a9c710b7e616683f194f18c43b43b869073a5ffcc6fc7a28c30723d6e58ce577356982d65b833a5a5c15bf9024b43d98",
            "Name": "cdetrio15",
            "Gas": 6000,
            "NoBenchmark": true
        }
        ]"#;

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct TestCase {
            input: String,
            expected: String,
        }

        let test_cases: Vec<TestCase> = serde_json::from_str(test_data).unwrap();

        test_cases.iter().for_each(|test| {
            let input = array_bytes::hex2bytes_unchecked(&test.input);
            let result = alt_bn128_multiplication(&input);
            assert!(result.is_ok());
            let result = result.unwrap();

            let expected = array_bytes::hex2bytes_unchecked(&test.expected);
            assert_eq!(result, expected);
        });
    }

    #[test]
    fn alt_bn128_pairing_test() {
        use serde::Deserialize;

        let test_data = r#"[
        {
            "Input": "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f593034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf704bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a416782bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c2032c61a830e3c17286de9462bf242fca2883585b93870a73853face6a6bf411198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "jeff1",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "2eca0c7238bf16e83e7a1e6c5d49540685ff51380f309842a98561558019fc0203d3260361bb8451de5ff5ecd17f010ff22f5c31cdf184e9020b06fa5997db841213d2149b006137fcfb23036606f848d638d576a120ca981b5b1a5f9300b3ee2276cf730cf493cd95d64677bbb75fc42db72513a4c1e387b476d056f80aa75f21ee6226d31426322afcda621464d0611d226783262e21bb3bc86b537e986237096df1f82dff337dd5972e32a8ad43e28a78a96a823ef1cd4debe12b6552ea5f06967a1237ebfeca9aaae0d6d0bab8e28c198c5a339ef8a2407e31cdac516db922160fa257a5fd5b280642ff47b65eca77e626cb685c84fa6d3b6882a283ddd1198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "jeff2",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "0f25929bcb43d5a57391564615c9e70a992b10eafa4db109709649cf48c50dd216da2f5cb6be7a0aa72c440c53c9bbdfec6c36c7d515536431b3a865468acbba2e89718ad33c8bed92e210e81d1853435399a271913a6520736a4729cf0d51eb01a9e2ffa2e92599b68e44de5bcf354fa2642bd4f26b259daa6f7ce3ed57aeb314a9a87b789a58af499b314e13c3d65bede56c07ea2d418d6874857b70763713178fb49a2d6cd347dc58973ff49613a20757d0fcc22079f9abd10c3baee245901b9e027bd5cfc2cb5db82d4dc9677ac795ec500ecd47deee3b5da006d6d049b811d7511c78158de484232fc68daf8a45cf217d1c2fae693ff5871e8752d73b21198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "jeff3",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "2f2ea0b3da1e8ef11914acf8b2e1b32d99df51f5f4f206fc6b947eae860eddb6068134ddb33dc888ef446b648d72338684d678d2eb2371c61a50734d78da4b7225f83c8b6ab9de74e7da488ef02645c5a16a6652c3c71a15dc37fe3a5dcb7cb122acdedd6308e3bb230d226d16a105295f523a8a02bfc5e8bd2da135ac4c245d065bbad92e7c4e31bf3757f1fe7362a63fbfee50e7dc68da116e67d600d9bf6806d302580dc0661002994e7cd3a7f224e7ddc27802777486bf80f40e4ca3cfdb186bac5188a98c45e6016873d107f5cd131f3a3e339d0375e58bd6219347b008122ae2b09e539e152ec5364e7e2204b03d11d3caa038bfc7cd499f8176aacbee1f39e4e4afc4bc74790a4a028aff2c3d2538731fb755edefd8cb48d6ea589b5e283f150794b6736f670d6a1033f9b46c6f5204f50813eb85c8dc4b59db1c5d39140d97ee4d2b36d99bc49974d18ecca3e7ad51011956051b464d9e27d46cc25e0764bb98575bd466d32db7b15f582b2d5c452b36aa394b789366e5e3ca5aabd415794ab061441e51d01e94640b7e3084a07e02c78cf3103c542bc5b298669f211b88da1679b0b64a63b7e0e7bfe52aae524f73a55be7fe70c7e9bfc94b4cf0da1213d2149b006137fcfb23036606f848d638d576a120ca981b5b1a5f9300b3ee2276cf730cf493cd95d64677bbb75fc42db72513a4c1e387b476d056f80aa75f21ee6226d31426322afcda621464d0611d226783262e21bb3bc86b537e986237096df1f82dff337dd5972e32a8ad43e28a78a96a823ef1cd4debe12b6552ea5f",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "jeff4",
            "Gas": 147000,
            "NoBenchmark": false
        },{
            "Input": "20a754d2071d4d53903e3b31a7e98ad6882d58aec240ef981fdf0a9d22c5926a29c853fcea789887315916bbeb89ca37edb355b4f980c9a12a94f30deeed30211213d2149b006137fcfb23036606f848d638d576a120ca981b5b1a5f9300b3ee2276cf730cf493cd95d64677bbb75fc42db72513a4c1e387b476d056f80aa75f21ee6226d31426322afcda621464d0611d226783262e21bb3bc86b537e986237096df1f82dff337dd5972e32a8ad43e28a78a96a823ef1cd4debe12b6552ea5f1abb4a25eb9379ae96c84fff9f0540abcfc0a0d11aeda02d4f37e4baf74cb0c11073b3ff2cdbb38755f8691ea59e9606696b3ff278acfc098fa8226470d03869217cee0a9ad79a4493b5253e2e4e3a39fc2df38419f230d341f60cb064a0ac290a3d76f140db8418ba512272381446eb73958670f00cf46f1d9e64cba057b53c26f64a8ec70387a13e41430ed3ee4a7db2059cc5fc13c067194bcc0cb49a98552fd72bd9edb657346127da132e5b82ab908f5816c826acb499e22f2412d1a2d70f25929bcb43d5a57391564615c9e70a992b10eafa4db109709649cf48c50dd2198a1f162a73261f112401aa2db79c7dab1533c9935c77290a6ce3b191f2318d198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "jeff5",
            "Gas": 147000,
            "NoBenchmark": false
        },{
            "Input": "1c76476f4def4bb94541d57ebba1193381ffa7aa76ada664dd31c16024c43f593034dd2920f673e204fee2811c678745fc819b55d3e9d294e45c9b03a76aef41209dd15ebff5d46c4bd888e51a93cf99a7329636c63514396b4a452003a35bf704bf11ca01483bfa8b34b43561848d28905960114c8ac04049af4b6315a416782bb8324af6cfc93537a2ad1a445cfd0ca2a71acd7ac41fadbf933c2a51be344d120a2a4cf30c1bf9845f20c6fe39e07ea2cce61f0c9bb048165fe5e4de877550111e129f1cf1097710d41c4ac70fcdfa5ba2023c6ff1cbeac322de49d1b6df7c103188585e2364128fe25c70558f1560f4f9350baf3959e603cc91486e110936198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000000",
            "Name": "jeff6",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "empty_data",
            "Gas": 45000,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000000",
            "Name": "one_point",
            "Gas": 79000,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "two_point_match_2",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "two_point_match_3",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "105456a333e6d636854f987ea7bb713dfd0ae8371a72aea313ae0c32c0bf10160cf031d41b41557f3e7e3ba0c51bebe5da8e6ecd855ec50fc87efcdeac168bcc0476be093a6d2b4bbf907172049874af11e1b6267606e00804d3ff0037ec57fd3010c68cb50161b7d1d96bb71edfec9880171954e56871abf3d93cc94d745fa114c059d74e5b6c4ec14ae5864ebe23a71781d86c29fb8fb6cce94f70d3de7a2101b33461f39d9e887dbb100f170a2345dde3c07e256d1dfa2b657ba5cd030427000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000021a2c3013d2ea92e13c800cde68ef56a294b883f6ac35d25f587c09b1b3c635f7290158a80cd3d66530f74dc94c94adb88f5cdb481acca997b6e60071f08a115f2f997f3dbd66a7afe07fe7862ce239edba9e05c5afff7f8a1259c9733b2dfbb929d1691530ca701b4a106054688728c9972c8512e9789e9567aae23e302ccd75",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "two_point_match_4",
            "Gas": 113000,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed275dc4a288d1afb3cbb1ac09187524c7db36395df7be3b99e673b13a075a65ec1d9befcd05a5323e6da4d435f3b617cdb3af83285c2df711ef39c01571827f9d",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "ten_point_match_1",
            "Gas": 385000,
            "NoBenchmark": false
        },{
            "Input": "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002203e205db4f19b37b60121b83a7333706db86431c6d835849957ed8c3928ad7927dc7234fd11d3e8c36c59277c3e6f149d5cd3cfa9a62aee49f8130962b4b3b9195e8aa5b7827463722b8c153931579d3505566b4edf48d498e185f0509de15204bb53b8977e5f92a0bc372742c4830944a59b4fe6b1c0466e2a6dad122b5d2e030644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd31a76dae6d3272396d0cbe61fced2bc532edac647851e3ac53ce1cc9c7e645a83198e9393920d483a7260bfb731fb5d25f1aa493335a9e71297e485b7aef312c21800deef121f1e76426a00665e5c4479674322d4f75edadd46debd5cd992f6ed090689d0585ff075ec9e99ad690c3395bc4b313370b38ef355acdadcd122975b12c85ea5db8c6deb4aab71808dcb408fe3d1e7690c43d37b4ce6cc0166fa7daa",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "ten_point_match_2",
            "Gas": 385000,
            "NoBenchmark": false
        },{
            "Input": "105456a333e6d636854f987ea7bb713dfd0ae8371a72aea313ae0c32c0bf10160cf031d41b41557f3e7e3ba0c51bebe5da8e6ecd855ec50fc87efcdeac168bcc0476be093a6d2b4bbf907172049874af11e1b6267606e00804d3ff0037ec57fd3010c68cb50161b7d1d96bb71edfec9880171954e56871abf3d93cc94d745fa114c059d74e5b6c4ec14ae5864ebe23a71781d86c29fb8fb6cce94f70d3de7a2101b33461f39d9e887dbb100f170a2345dde3c07e256d1dfa2b657ba5cd030427000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000021a2c3013d2ea92e13c800cde68ef56a294b883f6ac35d25f587c09b1b3c635f7290158a80cd3d66530f74dc94c94adb88f5cdb481acca997b6e60071f08a115f2f997f3dbd66a7afe07fe7862ce239edba9e05c5afff7f8a1259c9733b2dfbb929d1691530ca701b4a106054688728c9972c8512e9789e9567aae23e302ccd75",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000001",
            "Name": "ten_point_match_3",
            "Gas": 113000,
            "NoBenchmark": false
        }
        ]"#;

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct TestCase {
            input: String,
            expected: String,
        }

        let test_cases: Vec<TestCase> = serde_json::from_str(test_data).unwrap();

        test_cases.iter().for_each(|test| {
            let input = array_bytes::hex2bytes_unchecked(&test.input);
            let result = alt_bn128_pairing(&input);
            assert!(result.is_ok());
            let result = result.unwrap();

            let expected = array_bytes::hex2bytes_unchecked(&test.expected);
            assert_eq!(result, expected);
        });
    }
}
