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
    updated_on TIMESTAMP NOT NULL
);

-- The table storing slot information
CREATE TABLE slot (
    slot BIGINT PRIMARY KEY,
    parent BIGINT,
    status varchar(16) NOT NULL,
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
    updated_on TIMESTAMP NOT NULL
);

CREATE FUNCTION audit_account_update() RETURNS trigger AS $audit_account_update$
    BEGIN
		INSERT INTO account_audit (pubkey, owner, lamports, slot, executable, rent_epoch, data, updated_on)
            VALUES (OLD.pubkey, OLD.owner, OLD.lamports, OLD.slot,
                    OLD.executable, OLD.rent_epoch, OLD.data, OLD.updated_on);
        RETURN NEW;
    END;

$audit_account_update$ LANGUAGE plpgsql;

CREATE TRIGGER account_update_trigger AFTER UPDATE OR DELETE ON account
    FOR EACH ROW EXECUTE PROCEDURE audit_account_update();
