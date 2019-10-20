# solana CLI

The [solana-cli crate](https://crates.io/crates/solana-cli) provides a command-line interface tool for Solana

## Examples

### Get Pubkey

```bash
// Command
$ solana address

// Return
<PUBKEY>
```

### Airdrop SOL/Lamports

```bash
// Command
$ solana airdrop 2

// Return
"2.00000000 SOL"

// Command
$ solana airdrop 123 --lamports

// Return
"123 lamports"
```

### Get Balance

```bash
// Command
$ solana balance

// Return
"3.00050001 SOL"
```

### Confirm Transaction

```bash
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### Deploy program

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

### Unconditional Immediate Transfer

```bash
// Command
$ solana pay <PUBKEY> 123

// Return
<TX_SIGNATURE>
```

### Post-Dated Transfer

```bash
// Command
$ solana pay <PUBKEY> 123 \
    --after 2018-12-24T23:59:00 --require-timestamp-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

_`require-timestamp-from` is optional. If not provided, the transaction will expect a timestamp signed by this wallet's private key_

### Authorized Transfer

A third party must send a signature to unlock the lamports.

```bash
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

### Post-Dated and Authorized Transfer

```bash
// Command
$ solana pay <PUBKEY> 123 \
    --after 2018-12-24T23:59 --require-timestamp-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

### Multiple Witnesses

```bash
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --require-signature-from <PUBKEY>

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

### Cancelable Transfer

```bash
// Command
$ solana pay <PUBKEY> 123 \
    --require-signature-from <PUBKEY> \
    --cancelable

// Return
{signature: <TX_SIGNATURE>, processId: <PROCESS_ID>}
```

### Cancel Transfer

```bash
// Command
$ solana cancel <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

### Send Signature

```bash
// Command
$ solana send-signature <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

### Indicate Elapsed Time

Use the current system time:

```bash
// Command
$ solana send-timestamp <PUBKEY> <PROCESS_ID>

// Return
<TX_SIGNATURE>
```

Or specify some other arbitrary timestamp:

```bash
// Command
$ solana send-timestamp <PUBKEY> <PROCESS_ID> --date 2018-12-24T23:59:00

// Return
<TX_SIGNATURE>
```

## Usage
### solana-cli
```text
solana-cli 0.20.0
Blockchain, Rebuilt for Scale

USAGE:
    solana [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

SUBCOMMANDS:
    address                              Get your public key
    airdrop                              Request lamports
    balance                              Get your balance
    cancel                               Cancel a transfer
    claim-storage-reward                 Redeem storage reward credits
    cluster-version                      Get the version of the cluster entrypoint
    confirm                              Confirm transaction by signature
    create-archiver-storage-account    Create an archiver storage account
    create-stake-account                 Create a stake account
    create-validator-storage-account     Create a validator storage account
    create-vote-account                  Create a vote account
    deactivate-stake                     Deactivate the delegated stake from the stake account
    delegate-stake                       Delegate stake to a vote account
    deploy                               Deploy a program
    fees                                 Display current cluster fees
    get                                  Get cli config settings
    get-epoch-info                       Get information about the current epoch
    get-genesis-blockhash                Get the genesis blockhash
    get-slot                             Get current slot
    get-transaction-count                Get current transaction count
    help                                 Prints this message or the help of the given subcommand(s)
    pay                                  Send a payment
    ping                                 Submit transactions sequentially
    redeem-vote-credits                  Redeem credits in the stake account
    send-signature                       Send a signature to authorize a transfer
    send-timestamp                       Send a timestamp to unlock a transfer
    set                                  Set a cli config setting
    show-account                         Show the contents of an account
    show-stake-account                   Show the contents of a stake account
    show-storage-account                 Show the contents of a storage account
    show-validators                      Show information about the current validators
    show-vote-account                    Show the contents of a vote account
    stake-authorize-staker               Authorize a new stake signing keypair for the given stake account
    stake-authorize-withdrawer           Authorize a new withdraw signing keypair for the given stake account
    uptime                               Show the uptime of a validator, based on epoch voting history
    validator-info                       Publish/get Validator info on Solana
    vote-authorize-voter                 Authorize a new vote signing keypair for the given vote account
    vote-authorize-withdrawer            Authorize a new withdraw signing keypair for the given vote account
    withdraw-stake                       Withdraw the unstaked lamports from the stake account
```

#### solana-address
```text
solana-address 
Get your public key

USAGE:
    solana address [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-airdrop
```text
solana-airdrop 
Request lamports

USAGE:
    solana airdrop [OPTIONS] <AMOUNT> [UNIT]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>        Configuration file to use [default: ~/.config/solana/cli/config.yml]
        --drone-host <HOST>    Drone host to use [default: the --url host]
        --drone-port <PORT>    Drone port to use [default: 9900]
    -u, --url <URL>            JSON RPC URL for the solana cluster
    -k, --keypair <PATH>       /path/to/id.json

ARGS:
    <AMOUNT>    The airdrop amount to request (default unit SOL)
    <UNIT>      Specify unit to use for request and balance display [possible values: SOL, lamports]
```

#### solana-balance
```text
solana-balance 
Get your balance

USAGE:
    solana balance [FLAGS] [OPTIONS] [PUBKEY]

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PUBKEY>    The public key of the balance to check
```

#### solana-cancel
```text
solana-cancel 
Cancel a transfer

USAGE:
    solana cancel [OPTIONS] <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PROCESS ID>    The process id of the transfer to cancel
```

#### solana-claim-storage-reward
```text
solana-claim-storage-reward 
Redeem storage reward credits

USAGE:
    solana claim-storage-reward [OPTIONS] <NODE PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <NODE PUBKEY>               The node account to credit the rewards to
    <STORAGE ACCOUNT PUBKEY>    Storage account address to redeem credits for
```

#### solana-cluster-version
```text
solana-cluster-version 
Get the version of the cluster entrypoint

USAGE:
    solana cluster-version [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-confirm
```text
solana-confirm 
Confirm transaction by signature

USAGE:
    solana confirm [OPTIONS] <SIGNATURE>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <SIGNATURE>    The transaction signature to confirm
```

#### solana-create-archiver-storage-account
```text
solana-create-archiver-storage-account 
Create an archiver storage account

USAGE:
    solana create-archiver-storage-account [OPTIONS] <STORAGE ACCOUNT OWNER PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT OWNER PUBKEY>    
    <STORAGE ACCOUNT PUBKEY>          
```

#### solana-create-stake-account
```text
solana-create-stake-account 
Create a stake account

USAGE:
    solana create-stake-account [OPTIONS] <STAKE ACCOUNT> <AMOUNT> [UNIT]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --authorized-staker <PUBKEY>        Public key of authorized staker (defaults to cli config pubkey)
        --authorized-withdrawer <PUBKEY>    Public key of the authorized withdrawer (defaults to cli config pubkey)
    -C, --config <PATH>                     Configuration file to use [default:
                                            ~/.config/solana/cli/config.yml]
        --custodian <PUBKEY>                Identity of the custodian (can withdraw before lockup expires)
    -u, --url <URL>                         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>                    /path/to/id.json
        --lockup <SLOT>                     The slot height at which this account will be available for withdrawal

ARGS:
    <STAKE ACCOUNT>    Address of the stake account to fund (pubkey or keypair)
    <AMOUNT>           The amount of send to the vote account (default unit SOL)
    <UNIT>             Specify unit to use for request [possible values: SOL, lamports]
```

#### solana-create-validator-storage-account
```text
solana-create-validator-storage-account 
Create a validator storage account

USAGE:
    solana create-validator-storage-account [OPTIONS] <STORAGE ACCOUNT OWNER PUBKEY> <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT OWNER PUBKEY>    
    <STORAGE ACCOUNT PUBKEY>          
```

#### solana-create-vote-account
```text
solana-create-vote-account 
Create a vote account

USAGE:
    solana create-vote-account [OPTIONS] <VOTE ACCOUNT PUBKEY> <VALIDATOR PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --authorized-voter <PUBKEY>         Public key of the authorized voter (defaults to vote account)
        --authorized-withdrawer <PUBKEY>    Public key of the authorized withdrawer (defaults to cli config pubkey)
        --commission <NUM>                  The commission taken on reward redemption (0-255), default: 0
    -C, --config <PATH>                     Configuration file to use [default:
                                            ~/.config/solana/cli/config.yml]
    -u, --url <URL>                         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>                    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account address to fund
    <VALIDATOR PUBKEY>       Validator that will vote with this account
```

#### solana-deactivate-stake
```text
solana-deactivate-stake 
Deactivate the delegated stake from the stake account

USAGE:
    solana deactivate-stake [OPTIONS] <STAKE ACCOUNT>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>    Stake account to be deactivated.
```

#### solana-delegate-stake
```text
solana-delegate-stake 
Delegate stake to a vote account

USAGE:
    solana delegate-stake [OPTIONS] <STAKE ACCOUNT> <VOTE ACCOUNT>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>    Stake account to delegate
    <VOTE ACCOUNT>     The vote account to which the stake will be delegated
```

#### solana-deploy
```text
solana-deploy 
Deploy a program

USAGE:
    solana deploy [OPTIONS] <PATH TO PROGRAM>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PATH TO PROGRAM>    /path/to/program.o
```

#### solana-fees
```text
solana-fees 
Display current cluster fees

USAGE:
    solana fees [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-get
```text
solana-get 
Get cli config settings

USAGE:
    solana get [OPTIONS] [CONFIG_FIELD]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <CONFIG_FIELD>    Return a specific config setting [possible values: url, keypair]
```

#### solana-get-epoch-info
```text
solana-get-epoch-info 
Get information about the current epoch

USAGE:
    solana get-epoch-info [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-get-genesis-blockhash
```text
solana-get-genesis-blockhash 
Get the genesis blockhash

USAGE:
    solana get-genesis-blockhash [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-get-slot
```text
solana-get-slot 
Get current slot

USAGE:
    solana get-slot [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-get-transaction-count
```text
solana-get-transaction-count 
Get current transaction count

USAGE:
    solana get-transaction-count [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-help
```text
solana-help 
Prints this message or the help of the given subcommand(s)

USAGE:
    solana help [subcommand]...

ARGS:
    <subcommand>...    The subcommand whose help message to display
```

#### solana-pay
```text
solana-pay 
Send a payment

USAGE:
    solana pay [FLAGS] [OPTIONS] <PUBKEY> <AMOUNT> [--] [UNIT]

FLAGS:
        --cancelable    
    -h, --help          Prints help information
    -V, --version       Prints version information

OPTIONS:
    -C, --config <PATH>                         Configuration file to use [default:
                                                ~/.config/solana/cli/config.yml]
    -u, --url <URL>                             JSON RPC URL for the solana cluster
    -k, --keypair <PATH>                        /path/to/id.json
        --after <DATETIME>                      A timestamp after which transaction will execute
        --require-timestamp-from <PUBKEY>       Require timestamp from this third party
        --require-signature-from <PUBKEY>...    Any third party signatures required to unlock the lamports

ARGS:
    <PUBKEY>    The pubkey of recipient
    <AMOUNT>    The amount to send (default unit SOL)
    <UNIT>      Specify unit to use for request [possible values: SOL, lamports]
```

#### solana-ping
```text
solana-ping 
Submit transactions sequentially

USAGE:
    solana ping [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>         Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -c, --count <NUMBER>        Stop after submitting count transactions
    -i, --interval <SECONDS>    Wait interval seconds between submitting the next transaction [default: 2]
    -u, --url <URL>             JSON RPC URL for the solana cluster
    -k, --keypair <PATH>        /path/to/id.json
    -t, --timeout <SECONDS>     Wait up to timeout seconds for transaction confirmation [default: 10]
```

#### solana-redeem-vote-credits
```text
solana-redeem-vote-credits 
Redeem credits in the stake account

USAGE:
    solana redeem-vote-credits [OPTIONS] <STAKE ACCOUNT> <VOTE ACCOUNT>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>    Address of the stake account in which to redeem credits
    <VOTE ACCOUNT>     The vote account to which the stake is currently delegated.
```

#### solana-send-signature
```text
solana-send-signature 
Send a signature to authorize a transfer

USAGE:
    solana send-signature [OPTIONS] <PUBKEY> <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS ID>    The process id of the transfer to authorize
```

#### solana-send-timestamp
```text
solana-send-timestamp 
Send a timestamp to unlock a transfer

USAGE:
    solana send-timestamp [OPTIONS] <PUBKEY> <PROCESS ID>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>      Configuration file to use [default: ~/.config/solana/cli/config.yml]
        --date <DATETIME>    Optional arbitrary timestamp to apply
    -u, --url <URL>          JSON RPC URL for the solana cluster
    -k, --keypair <PATH>     /path/to/id.json

ARGS:
    <PUBKEY>        The pubkey of recipient
    <PROCESS ID>    The process id of the transfer to unlock
```

#### solana-set
```text
solana-set 
Set a cli config setting

USAGE:
    solana set [OPTIONS] <--url <URL>|--keypair <PATH>>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-show-account
```text
solana-show-account 
Show the contents of an account

USAGE:
    solana show-account [FLAGS] [OPTIONS] <ACCOUNT PUBKEY>

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
    -o, --output <FILE>     Write the account data to this file

ARGS:
    <ACCOUNT PUBKEY>    Account pubkey
```

#### solana-show-stake-account
```text
solana-show-stake-account 
Show the contents of a stake account

USAGE:
    solana show-stake-account [FLAGS] [OPTIONS] <STAKE ACCOUNT>

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>    Address of the stake account to display
```

#### solana-show-storage-account
```text
solana-show-storage-account 
Show the contents of a storage account

USAGE:
    solana show-storage-account [OPTIONS] <STORAGE ACCOUNT PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STORAGE ACCOUNT PUBKEY>    Storage account pubkey
```

#### solana-show-validators
```text
solana-show-validators 
Show information about the current validators

USAGE:
    solana show-validators [FLAGS] [OPTIONS]

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json
```

#### solana-show-vote-account
```text
solana-show-vote-account 
Show the contents of a vote account

USAGE:
    solana show-vote-account [FLAGS] [OPTIONS] <VOTE ACCOUNT PUBKEY>

FLAGS:
    -h, --help        Prints help information
        --lamports    Display balance in lamports instead of SOL
    -V, --version     Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account pubkey
```

#### solana-stake-authorize-staker
```text
solana-stake-authorize-staker 
Authorize a new stake signing keypair for the given stake account

USAGE:
    solana stake-authorize-staker [OPTIONS] <STAKE ACCOUNT> <AUTHORIZE PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>       Stake account in which to set the authorized staker
    <AUTHORIZE PUBKEY>    New authorized staker
```

#### solana-stake-authorize-withdrawer
```text
solana-stake-authorize-withdrawer 
Authorize a new withdraw signing keypair for the given stake account

USAGE:
    solana stake-authorize-withdrawer [OPTIONS] <STAKE ACCOUNT> <AUTHORIZE PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>       Stake account in which to set the authorized withdrawer
    <AUTHORIZE PUBKEY>    New authorized withdrawer
```

#### solana-uptime
```text
solana-uptime 
Show the uptime of a validator, based on epoch voting history

USAGE:
    solana uptime [FLAGS] [OPTIONS] <VOTE ACCOUNT PUBKEY>

FLAGS:
        --aggregate    Aggregate uptime data across span
    -h, --help         Prints help information
    -V, --version      Prints version information

OPTIONS:
    -C, --config <PATH>           Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>               JSON RPC URL for the solana cluster
    -k, --keypair <PATH>          /path/to/id.json
        --span <NUM OF EPOCHS>    Number of recent epochs to examine

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account pubkey
```

#### solana-validator-info
```text
solana-validator-info 
Publish/get Validator info on Solana

USAGE:
    solana validator-info [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

SUBCOMMANDS:
    get        Get and parse Solana Validator info
    help       Prints this message or the help of the given subcommand(s)
    publish    Publish Validator info on Solana
```

#### solana-vote-authorize-voter
```text
solana-vote-authorize-voter 
Authorize a new vote signing keypair for the given vote account

USAGE:
    solana vote-authorize-voter [OPTIONS] <VOTE ACCOUNT PUBKEY> <NEW VOTER PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>    Vote account in which to set the authorized voter
    <NEW VOTER PUBKEY>       New vote signer to authorize
```

#### solana-vote-authorize-withdrawer
```text
solana-vote-authorize-withdrawer 
Authorize a new withdraw signing keypair for the given vote account

USAGE:
    solana vote-authorize-withdrawer [OPTIONS] <VOTE ACCOUNT PUBKEY> <NEW WITHDRAWER PUBKEY>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <VOTE ACCOUNT PUBKEY>      Vote account in which to set the authorized withdrawer
    <NEW WITHDRAWER PUBKEY>    New withdrawer to authorize
```

#### solana-withdraw-stake
```text
solana-withdraw-stake 
Withdraw the unstaked lamports from the stake account

USAGE:
    solana withdraw-stake [OPTIONS] <STAKE ACCOUNT> <DESTINATION ACCOUNT> <AMOUNT> [UNIT]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -C, --config <PATH>     Configuration file to use [default: ~/.config/solana/cli/config.yml]
    -u, --url <URL>         JSON RPC URL for the solana cluster
    -k, --keypair <PATH>    /path/to/id.json

ARGS:
    <STAKE ACCOUNT>          Stake account from which to withdraw
    <DESTINATION ACCOUNT>    The account to which the lamports should be transfered
    <AMOUNT>                 The amount to withdraw from the stake account (default unit SOL)
    <UNIT>                   Specify unit to use for request [possible values: SOL, lamports]
```

