pub mod prelude {
    pub use crate::alt_bn128::compression::{
        alt_bn128_compression_size::*, consts::*, target_arch::*, AltBn128CompressionError,
    };
}

use thiserror::Error;

mod consts {
    pub const ALT_BN128_G1_COMPRESS: u64 = 0;
    pub const ALT_BN128_G1_DECOMPRESS: u64 = 1;
    pub const ALT_BN128_G2_COMPRESS: u64 = 2;
    pub const ALT_BN128_G2_DECOMPRESS: u64 = 3;
}

mod alt_bn128_compression_size {
    pub const G1: usize = 64;
    pub const G2: usize = 128;
    pub const G1_COMPRESSED: usize = 32;
    pub const G2_COMPRESSED: usize = 64;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AltBn128CompressionError {
    #[error("Unexpected error")]
    UnexpectedError,
    #[error("Failed to decompress g1")]
    G1DecompressionFailed,
    #[error("Failed to decompress g2")]
    G2DecompressionFailed,
    #[error("Failed to compress affine g1")]
    G1CompressionFailed,
    #[error("Failed to compress affine g2")]
    G2CompressionFailed,
    #[error("Invalid input size")]
    InvalidInputSize,
}

impl From<u64> for AltBn128CompressionError {
    fn from(v: u64) -> AltBn128CompressionError {
        match v {
            1 => AltBn128CompressionError::G1DecompressionFailed,
            2 => AltBn128CompressionError::G2DecompressionFailed,
            3 => AltBn128CompressionError::G1CompressionFailed,
            4 => AltBn128CompressionError::G2CompressionFailed,
            5 => AltBn128CompressionError::InvalidInputSize,
            _ => AltBn128CompressionError::UnexpectedError,
        }
    }
}

impl From<AltBn128CompressionError> for u64 {
    fn from(v: AltBn128CompressionError) -> u64 {
        match v {
            AltBn128CompressionError::G1DecompressionFailed => 1,
            AltBn128CompressionError::G2DecompressionFailed => 2,
            AltBn128CompressionError::G1CompressionFailed => 3,
            AltBn128CompressionError::G2CompressionFailed => 4,
            AltBn128CompressionError::InvalidInputSize => 5,
            AltBn128CompressionError::UnexpectedError => 0,
        }
    }
}

#[cfg(not(target_os = "solana"))]
mod target_arch {

    use {
        super::*,
        crate::alt_bn128::compression::alt_bn128_compression_size,
        ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate},
    };

    type G1 = ark_bn254::g1::G1Affine;
    type G2 = ark_bn254::g2::G2Affine;

    pub fn alt_bn128_g1_decompress(
        g1_bytes: &[u8],
    ) -> Result<[u8; alt_bn128_compression_size::G1], AltBn128CompressionError> {
        let g1_bytes: [u8; alt_bn128_compression_size::G1_COMPRESSED] = g1_bytes
            .try_into()
            .map_err(|_| AltBn128CompressionError::InvalidInputSize)?;
        if g1_bytes == [0u8; alt_bn128_compression_size::G1_COMPRESSED] {
            return Ok([0u8; alt_bn128_compression_size::G1]);
        }
        let decompressed_g1 = G1::deserialize_with_mode(
            convert_endianness::<32, 32>(&g1_bytes).as_slice(),
            Compress::Yes,
            Validate::No,
        )
        .map_err(|_| AltBn128CompressionError::G1DecompressionFailed)?;
        let mut decompressed_g1_bytes = [0u8; alt_bn128_compression_size::G1];
        decompressed_g1
            .x
            .serialize_with_mode(&mut decompressed_g1_bytes[..32], Compress::No)
            .map_err(|_| AltBn128CompressionError::G1DecompressionFailed)?;
        decompressed_g1
            .y
            .serialize_with_mode(&mut decompressed_g1_bytes[32..], Compress::No)
            .map_err(|_| AltBn128CompressionError::G1DecompressionFailed)?;
        Ok(convert_endianness::<32, 64>(&decompressed_g1_bytes))
    }

    pub fn alt_bn128_g1_compress(
        g1_bytes: &[u8],
    ) -> Result<[u8; alt_bn128_compression_size::G1_COMPRESSED], AltBn128CompressionError> {
        let g1_bytes: [u8; alt_bn128_compression_size::G1] = g1_bytes
            .try_into()
            .map_err(|_| AltBn128CompressionError::InvalidInputSize)?;
        if g1_bytes == [0u8; alt_bn128_compression_size::G1] {
            return Ok([0u8; alt_bn128_compression_size::G1_COMPRESSED]);
        }
        let g1 = G1::deserialize_with_mode(
            convert_endianness::<32, 64>(&g1_bytes).as_slice(),
            Compress::No,
            Validate::No,
        )
        .map_err(|_| AltBn128CompressionError::G1CompressionFailed)?;
        let mut g1_bytes = [0u8; alt_bn128_compression_size::G1_COMPRESSED];
        G1::serialize_compressed(&g1, g1_bytes.as_mut_slice())
            .map_err(|_| AltBn128CompressionError::G2CompressionFailed)?;
        Ok(convert_endianness::<32, 32>(&g1_bytes))
    }

    pub fn alt_bn128_g2_decompress(
        g2_bytes: &[u8],
    ) -> Result<[u8; alt_bn128_compression_size::G2], AltBn128CompressionError> {
        let g2_bytes: [u8; alt_bn128_compression_size::G2_COMPRESSED] = g2_bytes
            .try_into()
            .map_err(|_| AltBn128CompressionError::InvalidInputSize)?;
        if g2_bytes == [0u8; alt_bn128_compression_size::G2_COMPRESSED] {
            return Ok([0u8; alt_bn128_compression_size::G2]);
        }
        let decompressed_g2 =
            G2::deserialize_compressed(convert_endianness::<64, 64>(&g2_bytes).as_slice())
                .map_err(|_| AltBn128CompressionError::G2DecompressionFailed)?;
        let mut decompressed_g2_bytes = [0u8; alt_bn128_compression_size::G2];
        decompressed_g2
            .x
            .serialize_with_mode(&mut decompressed_g2_bytes[..64], Compress::No)
            .map_err(|_| AltBn128CompressionError::G2DecompressionFailed)?;
        decompressed_g2
            .y
            .serialize_with_mode(&mut decompressed_g2_bytes[64..128], Compress::No)
            .map_err(|_| AltBn128CompressionError::G2DecompressionFailed)?;
        Ok(convert_endianness::<64, 128>(&decompressed_g2_bytes))
    }

    pub fn alt_bn128_g2_compress(
        g2_bytes: &[u8],
    ) -> Result<[u8; alt_bn128_compression_size::G2_COMPRESSED], AltBn128CompressionError> {
        let g2_bytes: [u8; alt_bn128_compression_size::G2] = g2_bytes
            .try_into()
            .map_err(|_| AltBn128CompressionError::InvalidInputSize)?;
        if g2_bytes == [0u8; alt_bn128_compression_size::G2] {
            return Ok([0u8; alt_bn128_compression_size::G2_COMPRESSED]);
        }
        let g2 = G2::deserialize_with_mode(
            convert_endianness::<64, 128>(&g2_bytes).as_slice(),
            Compress::No,
            Validate::No,
        )
        .map_err(|_| AltBn128CompressionError::G2DecompressionFailed)?;
        let mut g2_bytes = [0u8; alt_bn128_compression_size::G2_COMPRESSED];
        G2::serialize_compressed(&g2, g2_bytes.as_mut_slice())
            .map_err(|_| AltBn128CompressionError::G2CompressionFailed)?;
        Ok(convert_endianness::<64, 64>(&g2_bytes))
    }

    pub fn convert_endianness<const CHUNK_SIZE: usize, const ARRAY_SIZE: usize>(
        bytes: &[u8; ARRAY_SIZE],
    ) -> [u8; ARRAY_SIZE] {
        let reversed: [_; ARRAY_SIZE] = bytes
            .chunks_exact(CHUNK_SIZE)
            .flat_map(|chunk| chunk.iter().rev().copied())
            .enumerate()
            .fold([0u8; ARRAY_SIZE], |mut acc, (i, v)| {
                acc[i] = v;
                acc
            });
        reversed
    }
}

#[cfg(target_os = "solana")]
mod target_arch {
    use {
        super::*,
        alt_bn128_compression_size::{G1, G1_COMPRESSED, G2, G2_COMPRESSED},
        prelude::*,
    };

    pub fn alt_bn128_g1_compress(
        input: &[u8],
    ) -> Result<[u8; G1_COMPRESSED], AltBn128CompressionError> {
        let mut result_buffer = [0; G1_COMPRESSED];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_compression(
                ALT_BN128_G1_COMPRESS,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer),
            error => Err(AltBn128CompressionError::from(error)),
        }
    }

    pub fn alt_bn128_g1_decompress(input: &[u8]) -> Result<[u8; G1], AltBn128CompressionError> {
        let mut result_buffer = [0; G1];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_compression(
                ALT_BN128_G1_DECOMPRESS,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer),
            error => Err(AltBn128CompressionError::from(error)),
        }
    }

    pub fn alt_bn128_g2_compress(
        input: &[u8],
    ) -> Result<[u8; G2_COMPRESSED], AltBn128CompressionError> {
        let mut result_buffer = [0; G2_COMPRESSED];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_compression(
                ALT_BN128_G2_COMPRESS,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer),
            error => Err(AltBn128CompressionError::from(error)),
        }
    }

    pub fn alt_bn128_g2_decompress(
        input: &[u8; G2_COMPRESSED],
    ) -> Result<[u8; G2], AltBn128CompressionError> {
        let mut result_buffer = [0; G2];
        let result = unsafe {
            crate::syscalls::sol_alt_bn128_compression(
                ALT_BN128_G2_DECOMPRESS,
                input as *const _ as *const u8,
                input.len() as u64,
                &mut result_buffer as *mut _ as *mut u8,
            )
        };

        match result {
            0 => Ok(result_buffer),
            error => Err(AltBn128CompressionError::from(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::alt_bn128::compression::target_arch::convert_endianness,
        ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate},
        std::ops::Neg,
        target_arch::{
            alt_bn128_g1_compress, alt_bn128_g1_decompress, alt_bn128_g2_compress,
            alt_bn128_g2_decompress,
        },
    };
    type G1 = ark_bn254::g1::G1Affine;
    type G2 = ark_bn254::g2::G2Affine;

    #[test]
    fn alt_bn128_g1_compression() {
        let g1_be = [
            45, 206, 255, 166, 152, 55, 128, 138, 79, 217, 145, 164, 25, 74, 120, 234, 234, 217,
            68, 149, 162, 44, 133, 120, 184, 205, 12, 44, 175, 98, 168, 172, 20, 24, 216, 15, 209,
            175, 106, 75, 147, 236, 90, 101, 123, 219, 245, 151, 209, 202, 218, 104, 148, 8, 32,
            254, 243, 191, 218, 122, 42, 81, 193, 84,
        ];
        let g1_le = convert_endianness::<32, 64>(&g1_be);
        let g1: G1 =
            G1::deserialize_with_mode(g1_le.as_slice(), Compress::No, Validate::No).unwrap();

        let g1_neg = g1.neg();
        let mut g1_neg_be = [0u8; 64];
        g1_neg
            .x
            .serialize_with_mode(&mut g1_neg_be[..32], Compress::No)
            .unwrap();
        g1_neg
            .y
            .serialize_with_mode(&mut g1_neg_be[32..64], Compress::No)
            .unwrap();
        let g1_neg_be: [u8; 64] = convert_endianness::<32, 64>(&g1_neg_be);

        let points = [(g1, g1_be), (g1_neg, g1_neg_be)];

        for (point, g1_be) in &points {
            let mut compressed_ref = [0u8; 32];
            G1::serialize_with_mode(point, compressed_ref.as_mut_slice(), Compress::Yes).unwrap();
            let compressed_ref: [u8; 32] = convert_endianness::<32, 32>(&compressed_ref);

            let decompressed = alt_bn128_g1_decompress(compressed_ref.as_slice()).unwrap();

            assert_eq!(
                alt_bn128_g1_compress(&decompressed).unwrap(),
                compressed_ref
            );
            assert_eq!(decompressed, *g1_be);
        }
    }

    #[test]
    fn alt_bn128_g2_compression() {
        let g2_be = [
            40, 57, 233, 205, 180, 46, 35, 111, 215, 5, 23, 93, 12, 71, 118, 225, 7, 46, 247, 147,
            47, 130, 106, 189, 184, 80, 146, 103, 141, 52, 242, 25, 0, 203, 124, 176, 110, 34, 151,
            212, 66, 180, 238, 151, 236, 189, 133, 209, 17, 137, 205, 183, 168, 196, 92, 159, 75,
            174, 81, 168, 18, 86, 176, 56, 16, 26, 210, 20, 18, 81, 122, 142, 104, 62, 251, 169,
            98, 141, 21, 253, 50, 130, 182, 15, 33, 109, 228, 31, 79, 183, 88, 147, 174, 108, 4,
            22, 14, 129, 168, 6, 80, 246, 254, 100, 218, 131, 94, 49, 247, 211, 3, 245, 22, 200,
            177, 91, 60, 144, 147, 174, 90, 17, 19, 189, 62, 147, 152, 18,
        ];
        let g2_le = convert_endianness::<64, 128>(&g2_be);
        let g2: G2 =
            G2::deserialize_with_mode(g2_le.as_slice(), Compress::No, Validate::No).unwrap();

        let g2_neg = g2.neg();
        let mut g2_neg_be = [0u8; 128];
        g2_neg
            .x
            .serialize_with_mode(&mut g2_neg_be[..64], Compress::No)
            .unwrap();
        g2_neg
            .y
            .serialize_with_mode(&mut g2_neg_be[64..128], Compress::No)
            .unwrap();
        let g2_neg_be: [u8; 128] = convert_endianness::<64, 128>(&g2_neg_be);

        let points = [(g2, g2_be), (g2_neg, g2_neg_be)];

        for (point, g2_be) in &points {
            let mut compressed_ref = [0u8; 64];
            G2::serialize_with_mode(point, compressed_ref.as_mut_slice(), Compress::Yes).unwrap();
            let compressed_ref: [u8; 64] = convert_endianness::<64, 64>(&compressed_ref);

            let decompressed = alt_bn128_g2_decompress(compressed_ref.as_slice()).unwrap();

            assert_eq!(
                alt_bn128_g2_compress(&decompressed).unwrap(),
                compressed_ref
            );
            assert_eq!(decompressed, *g2_be);
        }
    }

    #[test]
    fn alt_bn128_compression_g1_point_of_infitity() {
        let g1_bytes = vec![0u8; 64];
        let g1_compressed = alt_bn128_g1_compress(&g1_bytes).unwrap();
        let g1_decompressed = alt_bn128_g1_decompress(&g1_compressed).unwrap();
        assert_eq!(g1_bytes, g1_decompressed);
    }

    #[test]
    fn alt_bn128_compression_g2_point_of_infitity() {
        let g1_bytes = vec![0u8; 128];
        let g1_compressed = alt_bn128_g2_compress(&g1_bytes).unwrap();
        let g1_decompressed = alt_bn128_g2_decompress(&g1_compressed).unwrap();
        assert_eq!(g1_bytes, g1_decompressed);
    }
    #[test]
    fn alt_bn128_compression_pairing_test_input() {
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
        }

        let test_cases: Vec<TestCase> = serde_json::from_str(test_data).unwrap();

        test_cases.iter().for_each(|test| {
            let input = array_bytes::hex2bytes_unchecked(&test.input);
            let g1 = input[0..64].to_vec();
            let g1_compressed = alt_bn128_g1_compress(&g1).unwrap();
            assert_eq!(g1, alt_bn128_g1_decompress(&g1_compressed).unwrap());
            let g2 = input[64..192].to_vec();
            let g2_compressed = alt_bn128_g2_compress(&g2).unwrap();
            assert_eq!(g2, alt_bn128_g2_decompress(&g2_compressed).unwrap());
        });
    }
}
