import React from "react";

import { AddressLookupTableAccount, PublicKey } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { AddressLookupTableAccountInfo } from "validators/accounts/address-lookup-table";

export function LookupTableEntriesCard(
  params:
    | {
        parsedLookupTable: AddressLookupTableAccountInfo;
      }
    | {
        lookupTableAccountData: Uint8Array;
      }
) {
  const lookupTableState = React.useMemo(() => {
    if ("lookupTableAccountData" in params) {
      return AddressLookupTableAccount.deserialize(
        params.lookupTableAccountData
      );
    } else {
      return params.parsedLookupTable;
    }
  }, [params]);

  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h3 className="card-header-title">Lookup Table Entries</h3>
          </div>
        </div>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="w-1 text-muted">Index</th>
              <th className="text-muted">Address</th>
            </tr>
          </thead>
          <tbody className="list">
            {lookupTableState.addresses.length > 0 &&
              lookupTableState.addresses.map((entry: PublicKey, index) => {
                return renderRow(entry, index);
              })}
          </tbody>
        </table>
      </div>

      {lookupTableState.addresses.length === 0 && (
        <div className="card-footer">
          <div className="text-muted text-center">No entries found</div>
        </div>
      )}
    </div>
  );
}

const renderRow = (entry: PublicKey, index: number) => {
  return (
    <tr key={index}>
      <td className="w-1 font-monospace">{index}</td>
      <td className="font-monospace">
        <Address pubkey={entry} link />
      </td>
    </tr>
  );
};
