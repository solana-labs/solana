/**
 * This plugin implementation for PostgreSQL requires the following tables
 */
-- The table storing accounts


CREATE TABLE account (
    pubkey BYTEA PRIMARY KEY,
    owner BYTEA,
    lamports BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    executable BOOL NOT NULL,
    rent_epoch BIGINT NOT NULL,
    data BYTEA,
    write_version BIGINT NOT NULL,
    updated_on TIMESTAMP NOT NULL
);

-- The table storing slot information
CREATE TABLE slot (
    slot BIGINT PRIMARY KEY,
    parent BIGINT,
    status VARCHAR(16) NOT NULL,
    updated_on TIMESTAMP NOT NULL
);

-- Types for Transactions

Create TYPE "TransactionErrorCode" AS ENUM (
    'AccountInUse',
    'AccountLoadedTwice',
    'AccountNotFound',
    'ProgramAccountNotFound',
    'InsufficientFundsForFee',
    'InvalidAccountForFee',
    'AlreadyProcessed',
    'BlockhashNotFound',
    'InstructionError',
    'CallChainTooDeep',
    'MissingSignatureForFee',
    'InvalidAccountIndex',
    'SignatureFailure',
    'InvalidProgramForExecution',
    'SanitizeFailure',
    'ClusterMaintenance',
    'AccountBorrowOutstanding',
    'WouldExceedMaxAccountCostLimit',
    'WouldExceedMaxBlockCostLimit',
    'UnsupportedVersion',
    'InvalidWritableAccount',
    'WouldExceedMaxAccountDataCostLimit',
    'TooManyAccountLocks'
);

CREATE TYPE "TransactionError" AS (
    error_code "TransactionErrorCode",
    error_detail VARCHAR(256)
);

CREATE TYPE "CompiledInstruction" AS (
    program_id_index SMALLINT,
    accounts SMALLINT[],
    data BYTEA
);

CREATE TYPE "InnerInstructions" AS (
    index SMALLINT,
    instructions "CompiledInstruction"[]
);

CREATE TYPE "TransactionTokenBalance" AS (
    account_index SMALLINT,
    mint VARCHAR(44),
    ui_token_amount DOUBLE PRECISION,
    owner VARCHAR(44)
);

Create TYPE "RewardType" AS ENUM (
    'Fee',
    'Rent',
    'Staking',
    'Voting'
);

CREATE TYPE "Reward" AS (
    pubkey VARCHAR(44),
    lamports BIGINT,
    post_balance BIGINT,
    reward_type "RewardType",
    commission SMALLINT
);

CREATE TYPE "TransactionStatusMeta" AS (
    error "TransactionError",
    fee BIGINT,
    pre_balances BIGINT[],
    post_balances BIGINT[],
    inner_instructions "InnerInstructions"[],
    log_messages TEXT[],
    pre_token_balances "TransactionTokenBalance"[],
    post_token_balances "TransactionTokenBalance"[],
    rewards "Reward"[]
);

CREATE TYPE "TransactionMessageHeader" AS (
    num_required_signatures SMALLINT,
    num_readonly_signed_accounts SMALLINT,
    num_readonly_unsigned_accounts SMALLINT
);

CREATE TYPE "TransactionMessage" AS (
    header "TransactionMessageHeader",
    account_keys BYTEA[],
    recent_blockhash BYTEA,
    instructions "CompiledInstruction"[]
);

CREATE TYPE "TransactionMessageAddressTableLookup" AS (
    account_key BYTEA,
    writable_indexes SMALLINT[],
    readonly_indexes SMALLINT[]
);

CREATE TYPE "TransactionMessageV0" AS (
    header "TransactionMessageHeader",
    account_keys BYTEA[],
    recent_blockhash BYTEA,
    instructions "CompiledInstruction"[],
    address_table_lookups "TransactionMessageAddressTableLookup"[]
);

CREATE TYPE "LoadedAddresses" AS (
    writable BYTEA[],
    readonly BYTEA[]
);

CREATE TYPE "LoadedMessageV0" AS (
    message "TransactionMessageV0",
    loaded_addresses "LoadedAddresses"
);

-- The table storing transactions
CREATE TABLE transaction (
    slot BIGINT NOT NULL,
    signature BYTEA NOT NULL,
    is_vote BOOL NOT NULL,
    message_type SMALLINT, -- 0: legacy, 1: v0 message
    legacy_message "TransactionMessage",
    v0_loaded_message "LoadedMessageV0",
    signatures BYTEA[],
    message_hash BYTEA,
    meta "TransactionStatusMeta",
    updated_on TIMESTAMP NOT NULL,
    CONSTRAINT transaction_pk PRIMARY KEY (slot, signature)
);

-- The table storing block metadata
CREATE TABLE block (
    slot BIGINT PRIMARY KEY,
    blockhash VARCHAR(44),
    rewards "Reward"[],
    block_time BIGINT,
    block_height BIGINT,
    updated_on TIMESTAMP NOT NULL
);

/**
 * The following is for keeping historical data for accounts and is not required for plugin to work.
 */
-- The table storing historical data for accounts
CREATE TABLE account_audit (
    pubkey BYTEA,
    owner BYTEA,
    lamports BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    executable BOOL NOT NULL,
    rent_epoch BIGINT NOT NULL,
    data BYTEA,
    write_version BIGINT NOT NULL,
    updated_on TIMESTAMP NOT NULL
);

CREATE INDEX account_audit_account_key ON  account_audit (pubkey, write_version);

CREATE FUNCTION audit_account_update() RETURNS trigger AS $audit_account_update$
    BEGIN
		INSERT INTO account_audit (pubkey, owner, lamports, slot, executable, rent_epoch, data, write_version, updated_on)
            VALUES (OLD.pubkey, OLD.owner, OLD.lamports, OLD.slot,
                    OLD.executable, OLD.rent_epoch, OLD.data, OLD.write_version, OLD.updated_on);
        RETURN NEW;
    END;

$audit_account_update$ LANGUAGE plpgsql;

CREATE TRIGGER account_update_trigger AFTER UPDATE OR DELETE ON account
    FOR EACH ROW EXECUTE PROCEDURE audit_account_update();
