import React from "react";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { NonceAccount } from "validators/accounts/nonce";
import {
  AccountHeader,
  AccountAddressRow,
  AccountBalanceRow,
} from "components/common/Account";

export function NonceAccountSection({
  account,
  nonceAccount,
}: {
  account: Account;
  nonceAccount: NonceAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Nonce Account"
        refresh={() => refresh(account.pubkey, "parsed")}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
        <AccountBalanceRow account={account} />

        <tr>
          <td>Authority</td>
          <td className="text-lg-end">
            <Address pubkey={nonceAccount.info.authority} alignRight raw link />
          </td>
        </tr>

        <tr>
          <td>Blockhash</td>
          <td className="text-lg-end">
            <code>{nonceAccount.info.blockhash}</code>
          </td>
        </tr>

        <tr>
          <td>Fee</td>
          <td className="text-lg-end">
            {nonceAccount.info.feeCalculator.lamportsPerSignature} lamports per
            signature
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}
