# | source | this file

# shellcheck disable=SC2034
# shellcheck disable=SC2086

STAKE_PROGRAM_PUBKEY=Stake11111111111111111111111111111111111111
SYSTEM_PROGRAM_PUBKEY=11111111111111111111111111111111
VOTE_PROGRAM_PUBKEY=Vote111111111111111111111111111111111111111
STORAGE_PROGRAM_PUBKEY=Storage111111111111111111111111111111111111
CONFIG_PROGRAM_PUBKEY=Config1111111111111111111111111111111111111

function get_program_accounts {
  PROGRAM_NAME="$1"
  PROGRAM_PUBKEY="$2"
  URL="$3"

  if [[ -n "$4" ]] ; then
    JSON_OUTFILE="$4"
  else
    JSON_OUTFILE="${PROGRAM_NAME}_account_data.json"
  fi
  curl -s -X POST -H "Content-Type: application/json" -d \
    '{"jsonrpc":"2.0","id":1, "method":"getProgramAccounts", "params":["'$PROGRAM_PUBKEY'"]}' $URL | jq '.' \
    > $JSON_OUTFILE
}

function write_program_account_data_csv {
  PROGRAM_NAME="$1"
  if [[ -n "$2" ]] ; then
    JSON_INFILE="$2"
  else
    JSON_INFILE="${PROGRAM_NAME}_account_data.json"
  fi
  if [[ -n "$3" ]] ; then
    CSV_OUTFILE="$3"
  else
    CSV_OUTFILE="${PROGRAM_NAME}_account_data.csv"
  fi

  echo "Account_Pubkey,Lamports" > $CSV_OUTFILE
  # shellcheck disable=SC2002
  cat "$JSON_INFILE" | jq -r '(.result | .[]) | [.pubkey, (.account | .lamports)] | @csv' \
    >> $CSV_OUTFILE
}

function get_account_info {
  ACCOUNT_PUBKEY="$1"
  URL="$2"

  if [[ -n "$3" ]] ; then
    JSON_OUTFILE="$3"
  else
    JSON_OUTFILE="${ACCOUNT_PUBKEY}_account_info.json"
  fi
  curl -s -X POST -H "Content-Type: application/json" -d \
    '{"jsonrpc":"2.0","id":1, "method":"getAccountInfo", "params":["'$ACCOUNT_PUBKEY'"]}' $URL | jq '.' \
    > $JSON_OUTFILE
}
