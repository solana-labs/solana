import React from "react";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { TableCardBody } from "components/common/TableCardBody";
import {
  ConfigAccount,
  StakeConfigInfoAccount,
  ValidatorInfoAccount,
} from "validators/accounts/config";
import {
  AccountAddressRow,
  AccountBalanceRow,
  AccountHeader,
} from "components/common/Account";
import { PublicKey } from "@solana/web3.js";
import { Address } from "components/common/Address";

const MAX_SLASH_PENALTY = Math.pow(2, 8);

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
  configAccount: StakeConfigInfoAccount;
}) {
  const refresh = useFetchAccountInfo();

  const warmupCooldownFormatted = new Intl.NumberFormat("en-US", {
    style: "percent",
    maximumFractionDigits: 2,
  }).format(configAccount.info.warmupCooldownRate);

  const slashPenaltyFormatted = new Intl.NumberFormat("en-US", {
    style: "percent",
    maximumFractionDigits: 2,
  }).format(configAccount.info.slashPenalty / MAX_SLASH_PENALTY);

  return (
    <div className="card">
      <AccountHeader
        title="Stake Config"
        refresh={() => refresh(account.pubkey, "parsed")}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
        <AccountBalanceRow account={account} />

        <tr>
          <td>Warmup / Cooldown Rate</td>
          <td className="text-lg-end">{warmupCooldownFormatted}</td>
        </tr>

        <tr>
          <td>Slash Penalty</td>
          <td className="text-lg-end">{slashPenaltyFormatted}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function ValidatorInfoCard({
  account,
  configAccount,
}: {
  account: Account;
  configAccount: ValidatorInfoAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Validator Info"
        refresh={() => refresh(account.pubkey, "parsed")}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
        <AccountBalanceRow account={account} />

        {configAccount.info.configData.name && (
          <tr>
            <td>Name</td>
            <td className="text-lg-end">
              {configAccount.info.configData.name}
            </td>
          </tr>
        )}

        {configAccount.info.configData.keybaseUsername && (
          <tr>
            <td>Keybase Username</td>
            <td className="text-lg-end">
              {configAccount.info.configData.keybaseUsername}
            </td>
          </tr>
        )}

        {configAccount.info.configData.website && (
          <tr>
            <td>Website</td>
            <td className="text-lg-end">
              <a
                href={configAccount.info.configData.website}
                target="_blank"
                rel="noopener noreferrer"
              >
                {configAccount.info.configData.website}
              </a>
            </td>
          </tr>
        )}

        {configAccount.info.configData.details && (
          <tr>
            <td>Details</td>
            <td className="text-lg-end">
              {configAccount.info.configData.details}
            </td>
          </tr>
        )}

        {configAccount.info.keys && configAccount.info.keys.length > 1 && (
          <tr>
            <td>Signer</td>
            <td className="text-lg-end">
              <Address
                pubkey={new PublicKey(configAccount.info.keys[1].pubkey)}
                link
                alignRight
              />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
