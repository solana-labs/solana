#[link(name = "poh-simd")]
extern "C" {
    pub fn poh_verify_many_simd_sse2(hashes: *mut u8, num_hashes_arr: *const u64);

    pub fn poh_verify_many_simd_sse4(hashes: *mut u8, num_hashes_arr: *const u64);

    pub fn poh_verify_many_simd_avx1(hashes: *mut u8, num_hashes_arr: *const u64);

    pub fn poh_verify_many_simd_avx2(hashes: *mut u8, num_hashes_arr: *const u64);

    pub fn poh_verify_many_simd_avx512skx(hashes: *mut u8, num_hashes_arr: *const u64);
}
