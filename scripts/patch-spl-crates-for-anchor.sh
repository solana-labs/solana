spl_associated_token_account_version=
spl_memo_version=
spl_pod_version=
spl_token_version=
spl_token_2022_version=
spl_token_group_interface_version=
spl_token_metadata_interface_version=
spl_tlv_account_resolution_version=
spl_transfer_hook_interface_version=
spl_type_length_value_version=

get_spl_versions() {
    declare spl_dir="$1"
    spl_associated_token_account_version=$(readCargoVariable version "$spl_dir/associated-token-account/program/Cargo.toml")
    spl_memo_version=$(readCargoVariable version "$spl_dir/memo/program/Cargo.toml")
    spl_pod_version=$(readCargoVariable version "$spl_dir/libraries/pod/Cargo.toml")
    spl_token_version=$(readCargoVariable version "$spl_dir/token/program/Cargo.toml")
    spl_token_2022_version=$(readCargoVariable version "$spl_dir/token/program-2022/Cargo.toml"| head -c1) # only use the major version for convenience
    spl_token_group_interface_version=$(readCargoVariable version "$spl_dir/token-group/interface/Cargo.toml")
    spl_token_metadata_interface_version=$(readCargoVariable version "$spl_dir/token-metadata/interface/Cargo.toml")
    spl_tlv_account_resolution_version=$(readCargoVariable version "$spl_dir/libraries/tlv-account-resolution/Cargo.toml")
    spl_transfer_hook_interface_version=$(readCargoVariable version "$spl_dir/token/transfer-hook/interface/Cargo.toml")
    spl_type_length_value_version=$(readCargoVariable version "$spl_dir/libraries/type-length-value/Cargo.toml")
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

    sed -i -e "s#\(spl-associated-token-account = \"\)[^\"]*\(\"\)#\1$spl_associated_token_account_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-associated-token-account = { version = \"\)[^\"]*\(\"\)#\1$spl_associated_token_account_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-memo = \"\)[^\"]*\(\"\)#\1$spl_memo_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-memo = { version = \"\)[^\"]*\(\"\)#\1$spl_memo_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-pod = \"\)[^\"]*\(\"\)#\1$spl_pod_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-pod = { version = \"\)[^\"]*\(\"\)#\1$spl_pod_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token = \"\)[^\"]*\(\"\)#\1$spl_token_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token = { version = \"\)[^\"]*\(\"\)#\1$spl_token_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-2022 = \"\).*\(\"\)#\1$spl_token_2022_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-2022 = { version = \"\)[^\"]*\(\"\)#\1$spl_token_2022_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-group-interface = \"\)[^\"]*\(\"\)#\1=$spl_token_group_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-group-interface = { version = \"\)[^\"]*\(\"\)#\1=$spl_token_group_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-metadata-interface = \"\)[^\"]*\(\"\)#\1=$spl_token_metadata_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-token-metadata-interface = { version = \"\)[^\"]*\(\"\)#\1=$spl_token_metadata_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-tlv-account-resolution = \"\)[^\"]*\(\"\)#\1=$spl_tlv_account_resolution_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-tlv-account-resolution = { version = \"\)[^\"]*\(\"\)#\1=$spl_tlv_account_resolution_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-transfer-hook-interface = \"\)[^\"]*\(\"\)#\1=$spl_transfer_hook_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-transfer-hook-interface = { version = \"\)[^\"]*\(\"\)#\1=$spl_transfer_hook_interface_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-type-length-value = \"\)[^\"]*\(\"\)#\1=$spl_type_length_value_version\2#g" "${tomls[@]}" || return $?
    sed -i -e "s#\(spl-type-length-value = { version = \"\)[^\"]*\(\"\)#\1=$spl_type_length_value_version\2#g" "${tomls[@]}" || return $?

    # patch ahash. This is super brittle; putting here for convenience, since we are already iterating through the tomls
    ahash_minor_version="0.8"
    sed -i -e "s#\(ahash = \"\)[^\"]*\(\"\)#\1$ahash_minor_version\2#g" "${tomls[@]}" || return $?
}

patch_crates_io() {
    declare Cargo_toml="$1"
    declare spl_dir="$2"
    cat >> "$Cargo_toml" <<EOF
    spl-associated-token-account = { path = "$spl_dir/associated-token-account/program" }
    spl-memo = { path = "$spl_dir/memo/program" }
    spl-pod = { path = "$spl_dir/libraries/pod" }
    spl-token = { path = "$spl_dir/token/program" }
    # Avoid patching spl-token-2022 to avoid forcing anchor to use 4.0.1, which
    # doesn't work with the monorepo forcing 4.0.0. Allow the patching again once
    # the monorepo is on 4.0.1, or relax the dependency in the monorepo.
    #spl-token-2022 = { path = "$spl_dir/token/program-2022" }
    spl-token-group-interface = { path = "$spl_dir/token-group/interface" }
    spl-token-metadata-interface = { path = "$spl_dir/token-metadata/interface" }
    spl-tlv-account-resolution = { path = "$spl_dir/libraries/tlv-account-resolution" }
    spl-transfer-hook-interface = { path = "$spl_dir/token/transfer-hook/interface" }
    spl-type-length-value = { path = "$spl_dir/libraries/type-length-value" }
EOF
}
