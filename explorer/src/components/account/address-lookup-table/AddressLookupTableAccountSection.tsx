import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { SolBalance } from "utils";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { Address } from "components/common/Address";
import { AddressLookupTableAccount } from "@solana/web3.js";
import { Slot } from "components/common/Slot";

export function AddressLookupTableAccountSection({
  account,
  data,
}: {
  account: Account;
  data: Uint8Array;
}) {
  const lookupTableAccount = React.useMemo(() => {
    return new AddressLookupTableAccount({
      key: account.pubkey,
      state: AddressLookupTableAccount.deserialize(data),
    });
  }, [account, data]);
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Address Lookup Table Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey)}
        >
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-end">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-end text-uppercase">
            <SolBalance lamports={account.lamports || 0} />
          </td>
        </tr>
        <tr>
          <td>Activation Status</td>
          <td className="text-lg-end text-uppercase">
            {lookupTableAccount.isActive() ? "Active" : "Deactivated"}
          </td>
        </tr>
        <tr>
          <td>Last Extended Slot</td>
          <td className="text-lg-end">
            {lookupTableAccount.state.lastExtendedSlot === 0 ? (
              "None (Empty)"
            ) : (
              <Slot slot={lookupTableAccount.state.lastExtendedSlot} link />
            )}
          </td>
        </tr>
        <tr>
          <td>Authority</td>
          <td className="text-lg-end">
            {lookupTableAccount.state.authority === undefined ? (
              "None (Frozen)"
            ) : (
              <Address
                pubkey={lookupTableAccount.state.authority}
                alignRight
                link
              />
            )}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}
