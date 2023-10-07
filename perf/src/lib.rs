#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
pub mod cuda_runtime;
pub mod data_budget;
pub mod deduper;
pub mod discard;
pub mod packet;
pub mod perf_libs;
pub mod recycler;
pub mod recycler_cache;
pub mod sigverify;
pub mod test_tx;
pub mod thread;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

#[macro_use]
extern crate solana_metrics;

#[macro_use]
extern crate solana_frozen_abi_macro;

fn is_rosetta_emulated() -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::str::FromStr;
        std::process::Command::new("sysctl")
            .args(["-in", "sysctl.proc_translated"])
            .output()
            .map_err(|_| ())
            .and_then(|output| String::from_utf8(output.stdout).map_err(|_| ()))
            .and_then(|stdout| u8::from_str(stdout.trim()).map_err(|_| ()))
            .map(|enabled| enabled == 1)
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn report_target_features() {
    warn!(
        "CUDA is {}abled",
        if crate::perf_libs::api().is_some() {
            "en"
        } else {
            "dis"
        }
    );

    // Validator binaries built on a machine with AVX support will generate invalid opcodes
    // when run on machines without AVX causing a non-obvious process abort.  Instead detect
    // the mismatch and error cleanly.
    if !is_rosetta_emulated() {
        #[cfg(all(
            any(target_arch = "x86", target_arch = "x86_64"),
            build_target_feature_avx
        ))]
        {
            if is_x86_feature_detected!("avx") {
                info!("AVX detected");
            } else {
                error!(
                "Incompatible CPU detected: missing AVX support. Please build from source on the target"
            );
                std::process::abort();
            }
        }

        #[cfg(all(
            any(target_arch = "x86", target_arch = "x86_64"),
            build_target_feature_avx2
        ))]
        {
            if is_x86_feature_detected!("avx2") {
                info!("AVX2 detected");
            } else {
                error!(
                    "Incompatible CPU detected: missing AVX2 support. Please build from source on the target"
                );
                std::process::abort();
            }
        }
    }
}
