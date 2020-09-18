import React from "react";

import { Account, useFetchAccountInfo } from "providers/accounts";
import { TableCardBody } from "components/common/TableCardBody";
import { ConfigAccount } from "validators/accounts/config";
import {
  AccountAddressRow,
  AccountHeader,
  AccountOwnerRow,
} from "components/common/Account";

export function ConfigAccountSection({
  account,
  configAccount,
}: {
  account: Account;
  configAccount: ConfigAccount;
}) {
  switch (configAccount.type) {
    case "stakeConfig":
      return (
        <StakeConfigCard account={account} configAccount={configAccount} />
      );
    case "validatorInfo":
      return (
        <ValidatorInfoCard account={account} configAccount={configAccount} />
      );
  }
}

function StakeConfigCard({
  account,
  configAccount,
}: {
  account: Account;
  configAccount: ConfigAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Stake Config"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Warmup / Cooldown Rate</td>
          <td className="text-lg-right">
            {configAccount.info.warmupCooldownRate}
          </td>
        </tr>

        <tr>
          <td>Slash Penalty</td>
          <td className="text-lg-right">{configAccount.info.slashPenalty}</td>
        </tr>

        <AccountOwnerRow account={account} />
      </TableCardBody>
    </div>
  );
}

function ValidatorInfoCard({
  account,
  configAccount,
}: {
  account: Account;
  configAccount: ConfigAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Validator Info"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />

        {configAccount.info.configData.name && (
          <tr>
            <td>Name</td>
            <td className="text-lg-right">
              {configAccount.info.configData.name}
            </td>
          </tr>
        )}

        {configAccount.info.configData.keybaseUsername && (
          <tr>
            <td>Keybase Username</td>
            <td className="text-lg-right">
              {configAccount.info.configData.keybaseUsername}
            </td>
          </tr>
        )}

        {configAccount.info.configData.website && (
          <tr>
            <td>Website</td>
            <td className="text-lg-right">
              <a href={configAccount.info.configData.website}>
                {configAccount.info.configData.website}
              </a>
            </td>
          </tr>
        )}

        {configAccount.info.configData.details && (
          <tr>
            <td>Details</td>
            <td className="text-lg-right">
              {configAccount.info.configData.details}
            </td>
          </tr>
        )}

        <AccountOwnerRow account={account} />
      </TableCardBody>
    </div>
  );
}
