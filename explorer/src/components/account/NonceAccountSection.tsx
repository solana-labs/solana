import React from "react";

import { Account, useFetchAccountInfo } from "providers/accounts";

import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { NonceAccount } from "validators/accounts/nonce";

export function NonceAccountSection({
  account,
  nonceAccount,
}: {
  account: Account;
  nonceAccount: NonceAccount;
}) {
  return <NonceAccountCard account={account} nonceAccount={nonceAccount} />;
}

function NonceAccountCard({
  account,
  nonceAccount,
}: {
  account: Account;
  nonceAccount: NonceAccount;
}) {
  const { details } = account;
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Nonce Account</h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey)}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
      </div>
      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-right">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        
        <tr>
          <td>Authority</td>
          <td className="text-lg-right">
            <Address pubkey={nonceAccount.info.authority} alignRight raw link />
          </td>
        </tr>
        
        <tr>
          <td>Blockhash</td>
          <td className="text-lg-right">
            <code>{nonceAccount.info.blockhash}</code>
          </td>
        </tr>
        
        <tr>
          <td>Fee</td>
          <td className="text-lg-right">
            {nonceAccount.info.feeCalculator.lamportsPerSignature} lamports per
            signature
          </td>
        </tr>

        {details && (
          <tr>
            <td>Owner</td>
            <td className="text-lg-right">
              <Address pubkey={details.owner} alignRight link />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
