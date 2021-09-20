-- The table storing accounts
CREATE TABLE account (
    pubkey VARCHAR(50) PRIMARY KEY,
    owner VARCHAR(50),
    lamports BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    executable BOOL NOT NULL,
    rent_epoch BIGINT NOT NULL,
    hash VARCHAR(50) NOT NULL,
    data BYTEA,
    updated_on TIMESTAMP NOT NULL
);

-- The table storing historical data for accounts
CREATE TABLE account_audit (
	pubkey VARCHAR(50),
	owner VARCHAR(50),
	lamports BIGINT NOT NULL,
	slot BIGINT NOT NULL,
	executable BOOL NOT NULL,
	rent_epoch BIGINT NOT NULL,
	hash VARCHAR(50) NOT NULL,
	data BYTEA,
	updated_on TIMESTAMP NOT NULL,
	CONSTRAINT slot_pubkey PRIMARY KEY (slot, pubkey)
);

