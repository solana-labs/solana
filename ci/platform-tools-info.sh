#!/usr/bin/env bash
#
# Finds the version of platform-tools used by this source tree.
#
# stdout of this script may be eval-ed.
#

here="$(dirname "$0")"

SBF_TOOLS_VERSION=unknown

cargo_build_sbf_main="${here}/../sdk/cargo-build-sbf/src/main.rs"
if [[ -f "${cargo_build_sbf_main}" ]]; then
    version=$(sed -e 's/^.*platform_tools_version\s*=\s*String::from("\(v[0-9.]\+\)").*/\1/;t;d' "${cargo_build_sbf_main}")
    if [[ ${version} != '' ]]; then
        SBF_TOOLS_VERSION="${version}"
    else
        echo '--- unable to parse SBF_TOOLS_VERSION'
    fi
else
    echo "--- '${cargo_build_sbf_main}' not present"
fi

echo SBF_TOOLS_VERSION="${SBF_TOOLS_VERSION}"
