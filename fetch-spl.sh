#!/usr/bin/env bash
#
# Fetches the latest SPL programs and produces the safecoin-genesis command-line
# arguments needed to install them
#

set -e

fetch_program() {
  declare name=$1
  declare version=$2
  declare address=$3
  declare loader=$4

  declare so=spl_$name-$version.so

  genesis_args+=(--bpf-program "$address" "$loader" "$so")

  if [[ -r $so ]]; then
    return
  fi

  if [[ -r ~/.cache/solana-spl/$so ]]; then
    cp ~/.cache/solana-spl/"$so" "$so"
  else
    echo "Downloading $name $version"
    so_name="spl_${name//-/_}.so"
    (
      set -x
      curl -L --retry 5 --retry-delay 2 --retry-connrefused \
        -o "$so" \
        "https://github.com/solana-labs/solana-program-library/releases/download/$name-v$version/$so_name"
    )

    mkdir -p ~/.cache/solana-spl
    cp "$so" ~/.cache/solana-spl/"$so"
  fi

}

fetch_program token 3.1.0 HMGr16f8Ct1Zeb9TGPypt9rPgzCkmhCQB8Not8vwiPW1 BPFLoader2111111111111111111111111111111111
fetch_program memo  1.0.0 4DDUJ1rA8Vd7e6SFWanf4V8JnsfapjCGNutQYw8Vtt45 BPFLoader1111111111111111111111111111111111
fetch_program memo  3.0.0 MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr BPFLoader2111111111111111111111111111111111
fetch_program associated-token-account 1.0.1 PUFQTv9BK3ax6bKPFnyjBTbVa3782mcfvb22TZovvrm BPFLoader2111111111111111111111111111111111
fetch_program feature-proposal 1.0.0 BKCvVdwmY6zQQyWijdMC2vjtYvCq9Q913yvvNLvjVSMv BPFLoader2111111111111111111111111111111111

echo "${genesis_args[@]}" > spl-genesis-args.sh

echo
echo "Available SPL programs:"
ls -l spl_*.so

echo
echo "safecoin-genesis command-line arguments (spl-genesis-args.sh):"
cat spl-genesis-args.sh
