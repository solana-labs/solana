/**
 * Script for cleaning up the schema for PostgreSQL used for the AccountsDb plugin.
 */

DROP TRIGGER account_update_trigger ON account;
DROP FUNCTION audit_account_update;
DROP TABLE account_audit;
DROP TABLE account;
DROP TABLE slot;
