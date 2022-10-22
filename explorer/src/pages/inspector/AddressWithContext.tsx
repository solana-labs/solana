import React from "react";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { Address } from "components/common/Address";
import {
  Account,
  useAccountInfo,
  useAddressLookupTable,
  useFetchAccountInfo,
} from "providers/accounts";
import { ClusterStatus, useCluster } from "providers/cluster";
import { addressLabel } from "utils/tx";
import { lamportsToSolString } from "utils";

type AccountValidator = (account: Account) => string | undefined;

export const createFeePayerValidator = (
  feeLamports: number
): AccountValidator => {
  return (account: Account): string | undefined => {
    if (account.lamports === 0) return "Account doesn't exist";
    if (!account.owner.equals(SystemProgram.programId))
      return "Only system-owned accounts can pay fees";
    // TODO: Actually nonce accounts can pay fees too
    if (account.space > 0) return "Only unallocated accounts can pay fees";
    if (account.lamports < feeLamports) {
      return "Insufficient funds for fees";
    }
    return;
  };
};

export const programValidator = (account: Account): string | undefined => {
  if (account.lamports === 0) return "Account doesn't exist";
  if (!account.executable) return "Only executable accounts can be invoked";
  return;
};

export function AddressFromLookupTableWithContext({
  lookupTableKey,
  lookupTableIndex,
}: {
  lookupTableKey: PublicKey;
  lookupTableIndex: number;
}) {
  const lookupTable = useAddressLookupTable(lookupTableKey.toBase58());
  const fetchAccountInfo = useFetchAccountInfo();
  React.useEffect(() => {
    if (!lookupTable) fetchAccountInfo(lookupTableKey);
  }, [lookupTableKey, lookupTable, fetchAccountInfo]);

  let pubkey;
  if (!lookupTable) {
    return (
      <span className="text-muted">
        <span className="spinner-grow spinner-grow-sm me-2"></span>
        Loading
      </span>
    );
  } else if (typeof lookupTable === "string") {
    return <div>Invalid Lookup Table</div>;
  } else if (lookupTableIndex < lookupTable.state.addresses.length) {
    pubkey = lookupTable.state.addresses[lookupTableIndex];
  } else {
    return <div>Invalid Lookup Table Index</div>;
  }

  return (
    <div className="d-flex align-items-end flex-column">
      <Address pubkey={pubkey} link />
      <AccountInfo pubkey={pubkey} />
    </div>
  );
}

export function AddressWithContext({
  pubkey,
  validator,
}: {
  pubkey: PublicKey;
  validator?: AccountValidator;
}) {
  return (
    <div className="d-flex align-items-end flex-column">
      <Address pubkey={pubkey} link />
      <AccountInfo pubkey={pubkey} validator={validator} />
    </div>
  );
}

function AccountInfo({
  pubkey,
  validator,
}: {
  pubkey: PublicKey;
  validator?: AccountValidator;
}) {
  const address = pubkey.toBase58();
  const fetchAccount = useFetchAccountInfo();
  const info = useAccountInfo(address);
  const { cluster, status } = useCluster();

  // Fetch account on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected && pubkey) {
      fetchAccount(pubkey);
    }
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  const account = info?.data;
  if (!account)
    return (
      <span className="text-muted">
        <span className="spinner-grow spinner-grow-sm me-2"></span>
        Loading
      </span>
    );

  const errorMessage = validator && validator(account);
  if (errorMessage) return <span className="text-warning">{errorMessage}</span>;

  if (account.lamports === 0) {
    return <span className="text-muted">Account doesn't exist</span>;
  }

  const ownerAddress = account.owner.toBase58();
  const ownerLabel = addressLabel(ownerAddress, cluster);

  return (
    <span className="text-muted">
      {`Owned by ${ownerLabel || ownerAddress}.`}
      {` Balance is ${lamportsToSolString(account.lamports)} SOL.`}
      {` Size is ${new Intl.NumberFormat("en-US").format(
        account.space
      )} byte(s).`}
    </span>
  );
}
