# source this file

update_solana_dependencies() {
  declare project_root="$1"
  declare solana_ver="$2"
  declare tomls=()
  while IFS='' read -r line; do tomls+=("$line"); done < <(find "$project_root" -name Cargo.toml)

  sed -i -e "s#\(solana-program = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-program = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-program-test = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-program-test = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-sdk = \"\).*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-sdk = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-client = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-client = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-cli-config = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-cli-config = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-clap-utils = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-clap-utils = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-account-decoder = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-account-decoder = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-faucet = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-faucet = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-zk-token-sdk = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
  sed -i -e "s#\(solana-zk-token-sdk = { version = \"\)[^\"]*\(\"\)#\1=$solana_ver\2#g" "${tomls[@]}" || return $?
}

patch_crates_io_solana() {
  declare Cargo_toml="$1"
  declare solana_dir="$2"
  cat >> "$Cargo_toml" <<EOF
[patch.crates-io]
EOF
patch_crates_io_solana_no_header "$Cargo_toml" "$solana_dir"
}

patch_crates_io_solana_no_header() {
  declare Cargo_toml="$1"
  declare solana_dir="$2"
  cat >> "$Cargo_toml" <<EOF
solana-account-decoder = { path = "$solana_dir/account-decoder" }
solana-clap-utils = { path = "$solana_dir/clap-utils" }
solana-client = { path = "$solana_dir/client" }
solana-cli-config = { path = "$solana_dir/cli-config" }
solana-program = { path = "$solana_dir/sdk/program" }
solana-program-test = { path = "$solana_dir/program-test" }
solana-sdk = { path = "$solana_dir/sdk" }
solana-faucet = { path = "$solana_dir/faucet" }
solana-zk-token-sdk = { path = "$solana_dir/zk-token-sdk" }
EOF
}
