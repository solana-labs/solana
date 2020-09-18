import { Address } from "components/common/Address";
import React from "react";
import { ConfigAccount, ConfigKey } from "validators/accounts/config";
import { PublicKey } from "@solana/web3.js";

export function KeysCard({ configAccount }: { configAccount: ConfigAccount }) {
  return (
    <div className="card">
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h3 className="card-header-title">Public Keys</h3>
          </div>
        </div>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Public Key</th>
              <th className="text-muted">Signer</th>
            </tr>
          </thead>
          <tbody className="list">
            {configAccount.info.keys.map((entry: ConfigKey) => {
              return renderAccountRow(entry);
            })}
          </tbody>
        </table>
      </div>

      <div className="card-footer">
        <div className="text-muted text-center"></div>
      </div>
    </div>
  );
}

const renderAccountRow = (key: ConfigKey) => {
  return (
    <tr key={key.pubkey}>
      <td className="text-monospace">
        <Address pubkey={new PublicKey(key.pubkey)} link />
      </td>
      <td className="text-monospace">
        <code>{key.signer ? "true" : "false"}</code>
      </td>
    </tr>
  );
};
