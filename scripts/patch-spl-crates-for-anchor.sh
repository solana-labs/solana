spl_memo_version=
spl_token_version=
spl_token_2022_version=
spl_tlv_account_resolution_verison=
spl_transfer_hook_interface_version=

get_spl_versions() {
    declare spl_dir="$1"
    spl_memo_version=$(readCargoVariable version "$spl_dir/memo/program/Cargo.toml")
    spl_token_version=$(readCargoVariable version "$spl_dir/token/program/Cargo.toml")
    spl_token_2022_version=$(readCargoVariable version "$spl_dir/token/program-2022/Cargo.toml"| head -c1) # only use the major version for convenience
    spl_tlv_account_resolution_verison=$(readCargoVariable version "$spl_dir/libraries/tlv-account-resolution/Cargo.toml")
    spl_transfer_hook_interface_version=$(readCargoVariable version "$spl_dir/token/transfer-hook/interface/Cargo.toml")
}

patch_spl_crates() {
    declare project_root="$1"
    declare Cargo_toml="$2"
    declare spl_dir="$3"
    update_spl_dependencies "$project_root"
    patch_crates_io "$Cargo_toml" "$spl_dir"
}

update_spl_dependencies() {
    declare project_root="$1"
    declare tomls=()
    while IFS='' read -r line; do tomls+=("$line"); done < <(find "$project_root" -name Cargo.toml)

    sed -i -e "s#\(spl-memo = \"\)[^\"]*\(\"\)#\1$spl_memo_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-memo = { version = \"\)[^\"]*\(\"\)#\1$spl_memo_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token = \"\)[^\"]*\(\"\)#\1$spl_token_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token = { version = \"\)[^\"]*\(\"\)#\1$spl_token_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-2022 = \"\).*\(\"\)#\1$spl_token_2022_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-2022 = { version = \"\)[^\"]*\(\"\)#\1$spl_token_2022_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-tlv-account-resolution = \"\)[^\"]*\(\"\)#\1=$spl_tlv_account_resolution_verison\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-tlv-account-resolution = { version = \"\)[^\"]*\(\"\)#\1=$spl_tlv_account_resolution_verison\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-transfer-hook-interface = \"\)[^\"]*\(\"\)#\1=$spl_transfer_hook_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-transfer-hook-interface = { version = \"\)[^\"]*\(\"\)#\1=$spl_transfer_hook_interface_version\2#g" "${tomls[@]}" || return $?

    # patch ahash. This is super brittle; putting here for convenience, since we are already iterating through the tomls
    ahash_minor_version="0.8"
    sed -i -e "s#\(ahash = \"\)[^\"]*\(\"\)#\1$ahash_minor_version\2#g" "${tomls[@]}" || return $?
}

patch_crates_io() {
    declare Cargo_toml="$1"
    declare spl_dir="$2"
    cat >> "$Cargo_toml" <<EOF
    spl-memo = { path = "$spl_dir/memo/program" }
    spl-token = { path = "$spl_dir/token/program" }
    spl-token-2022 = { path = "$spl_dir/token/program-2022" }
    spl-tlv-account-resolution = { path = "$spl_dir/libraries/tlv-account-resolution" }
    spl-transfer-hook-interface = { path = "$spl_dir/token/transfer-hook/interface" }
EOF
}
