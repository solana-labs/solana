/**
 * Script for cleaning up the schema for PostgreSQL used for the AccountsDb plugin.
 */

DROP TRIGGER account_update_trigger ON account;
DROP FUNCTION audit_account_update;
DROP TABLE account_audit;
DROP TABLE account;
DROP TABLE slot;
DROP TABLE transaction;

DROP TYPE "MappedMessage";
DROP TYPE "MappedAddresses";
DROP TYPE "TransactionMessageV0";
DROP TYPE "AddressMapIndexes";
DROP TYPE "TransactionMessage";
DROP TYPE "TransactionMessageHeader";
DROP TYPE "TransactionStatusMeta";
DROP TYPE "Rewards";
DROP TYPE "TransactionTokenBalance";
DROP TYPE "InnerInstructions";
DROP TYPE "CompiledInstruction";
