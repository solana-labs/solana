

PKG_CONFIG_PATH=/opt/rh/gcc-toolset-12/root/usr/lib64/pkgconfig:/usr/lib64/pkgconfig
export PKG_CONFIG_PATH

# ./cargo nightly clean
./cargo nightly build

#./cargo nightly test --package solana-sdk --lib -- ed25519_instruction::test::test_message_data_offsets --nocapture  > out

export RUST_BACKTRACE=1
#./cargo nightly test --package solana-runtime --lib -- system_instruction_processor::tests::test_create_from_account_is_nonce_fail --nocapture > out

./cargo nightly test --workspace --lib -- tests --nocapture > out
grep test_case_json out | sed -e 's/.*test_case_json//' -e 's/$/,/' | sort -u > ../firedancer-testbins/v14-tests.json

export MAINNET=1
./cargo nightly test --workspace --lib -- tests --nocapture > out
grep test_case_json out | sed -e 's/.*test_case_json//' -e 's/$/,/' | sort -u > ../firedancer-testbins/v14-mainnet.json

# grep 'test_sign {' out | sed -e 's/.*test_sign {/{/' -e 's/$/,/' -e 's///' | sort > signs.json

# grep test_transfer_lamports out.json
