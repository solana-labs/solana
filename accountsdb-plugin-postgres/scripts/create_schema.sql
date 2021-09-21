/**
 * This plugin implementation for PostgreSQL requires the following tables
 */
-- The table storing accounts

CREATE TYPE slot_status as ENUM ('processed', 'rooted', 'confirmed');

CREATE TABLE account (
    pubkey VARCHAR(50) PRIMARY KEY,
    owner VARCHAR(50),
    lamports BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    executable BOOL NOT NULL,
    rent_epoch BIGINT NOT NULL,
    hash VARCHAR(50),
    data BYTEA,
    updated_on TIMESTAMP NOT NULL
);

-- The table storing slot information
CREATE TABLE slot (
    slot BIGINT PRIMARY KEY,
    parent BIGINT,
    status slot_status,
    updated_on TIMESTAMP NOT NULL
);

/**
 * The following is for keeping historical data for accounts and is not required for plugin to work.
 */
-- The table storing historical data for accounts
CREATE TABLE account_audit (
    pubkey VARCHAR(50),
    owner VARCHAR(50),
    lamports BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    executable BOOL NOT NULL,
    rent_epoch BIGINT NOT NULL,
    hash VARCHAR(50),
    data BYTEA,
    updated_on TIMESTAMP NOT NULL,
    CONSTRAINT slot_pubkey PRIMARY KEY (slot, pubkey)
);

