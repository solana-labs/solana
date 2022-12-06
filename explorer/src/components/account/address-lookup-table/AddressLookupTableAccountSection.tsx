import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { SolBalance } from "components/common/SolBalance";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { Address } from "components/common/Address";
import { AddressLookupTableAccount } from "@solana/web3.js";
import { Slot } from "components/common/Slot";
import { AddressLookupTableAccountInfo } from "validators/accounts/address-lookup-table";

export function AddressLookupTableAccountSection(
  params:
    | {
        account: Account;
        data: Uint8Array;
      }
    | {
        account: Account;
        lookupTableAccount: AddressLookupTableAccountInfo;
      }
) {
  const account = params.account;
  const lookupTableState = React.useMemo(() => {
    if ("data" in params) {
      return AddressLookupTableAccount.deserialize(params.data);
    } else {
      return params.lookupTableAccount;
    }
  }, [params]);
  const lookupTableAccount = new AddressLookupTableAccount({
    key: account.pubkey,
    state: lookupTableState,
  });
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Address Lookup Table Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey, "parsed")}
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
            <SolBalance lamports={account.lamports} />
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
