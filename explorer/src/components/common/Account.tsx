import React from "react";
import { Address } from "./Address";
import { Account } from "providers/accounts";
import { SolBalance } from "utils";

type AccountHeaderProps = {
  title: string;
  refresh: Function;
};

type AccountProps = {
  account: Account;
};

export function AccountHeader({ title, refresh }: AccountHeaderProps) {
  return (
    <div className="card-header align-items-center">
      <h3 className="card-header-title">{title}</h3>
      <button className="btn btn-white btn-sm" onClick={() => refresh()}>
        <span className="fe fe-refresh-cw me-2"></span>
        Refresh
      </button>
    </div>
  );
}

export function AccountAddressRow({ account }: AccountProps) {
  return (
    <tr>
      <td>Address</td>
      <td className="text-lg-end">
        <Address pubkey={account.pubkey} alignRight raw />
      </td>
    </tr>
  );
}

export function AccountBalanceRow({ account }: AccountProps) {
  const { lamports } = account;
  return (
    <tr>
      <td>Balance (SOL)</td>
      <td className="text-lg-end text-uppercase">
        <SolBalance lamports={lamports} />
      </td>
    </tr>
  );
}
