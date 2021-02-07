import React from "react";
import { Account, useFetchAccountInfo } from "providers/accounts";
import {
  TokenAccount,
  MintAccountInfo,
  TokenAccountInfo,
  MultisigAccountInfo,
} from "validators/accounts/token";
import { coerce } from "superstruct";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { UnknownAccountCard } from "./UnknownAccountCard";
import { TokenRegistry } from "tokenRegistry";
import { useCluster } from "providers/cluster";
import { normalizeTokenAmount } from "utils";
import { addressLabel } from "utils/tx";
import { reportError } from "utils/sentry";

export function TokenAccountSection({
  account,
  tokenAccount,
}: {
  account: Account;
  tokenAccount: TokenAccount;
}) {
  try {
    switch (tokenAccount.type) {
      case "mint": {
        const info = coerce(tokenAccount.info, MintAccountInfo);
        return <MintAccountCard account={account} info={info} />;
      }
      case "account": {
        const info = coerce(tokenAccount.info, TokenAccountInfo);
        return <TokenAccountCard account={account} info={info} />;
      }
      case "multisig": {
        const info = coerce(tokenAccount.info, MultisigAccountInfo);
        return <MultisigAccountCard account={account} info={info} />;
      }
    }
  } catch (err) {
    reportError(err, {
      address: account.pubkey.toBase58(),
    });
  }
  return <UnknownAccountCard account={account} />;
}

function MintAccountCard({
  account,
  info,
}: {
  account: Account;
  info: MintAccountInfo;
}) {
  const { cluster } = useCluster();
  const mintAddress = account.pubkey.toBase58();
  const fetchInfo = useFetchAccountInfo();
  const refresh = () => fetchInfo(account.pubkey);

  const tokenInfo = TokenRegistry.get(mintAddress, cluster);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          {tokenInfo ? "Overview" : "Token Mint"}
        </h3>
        <button className="btn btn-white btn-sm" onClick={refresh}>
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
          <td>
            {info.mintAuthority === null ? "Fixed Supply" : "Current Supply"}
          </td>
          <td className="text-lg-right">
            {normalizeTokenAmount(info.supply, info.decimals).toFixed(
              info.decimals
            )}
          </td>
        </tr>
        {tokenInfo?.website && (
          <tr>
            <td>Website</td>
            <td className="text-lg-right">
              <a
                rel="noopener noreferrer"
                target="_blank"
                href={tokenInfo.website}
              >
                {tokenInfo.website}
                <span className="fe fe-external-link ml-2"></span>
              </a>
            </td>
          </tr>
        )}
        {info.mintAuthority && (
          <tr>
            <td>Mint Authority</td>
            <td className="text-lg-right">
              <Address pubkey={info.mintAuthority} alignRight link />
            </td>
          </tr>
        )}
        {info.freezeAuthority && (
          <tr>
            <td>Freeze Authority</td>
            <td className="text-lg-right">
              <Address pubkey={info.freezeAuthority} alignRight link />
            </td>
          </tr>
        )}
        <tr>
          <td>Decimals</td>
          <td className="text-lg-right">{info.decimals}</td>
        </tr>
        {!info.isInitialized && (
          <tr>
            <td>Status</td>
            <td className="text-lg-right">Uninitialized</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function TokenAccountCard({
  account,
  info,
}: {
  account: Account;
  info: TokenAccountInfo;
}) {
  const refresh = useFetchAccountInfo();
  const { cluster } = useCluster();

  const label = addressLabel(account.pubkey.toBase58(), cluster);

  let unit, balance;
  if (info.isNative) {
    unit = "SAFE";
    balance = (
      <>
        ◎
        <span className="text-monospace">
          {new Intl.NumberFormat("en-US", { maximumFractionDigits: 9 }).format(
            info.tokenAmount.uiAmount
          )}
        </span>
      </>
    );
  } else {
    balance = <>{info.tokenAmount.uiAmount}</>;
    unit = TokenRegistry.get(info.mint.toBase58(), cluster)?.symbol || "tokens";
  }

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Token Account
        </h3>
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
        {label && (
          <tr>
            <td>Address Label</td>
            <td className="text-lg-right">{label}</td>
          </tr>
        )}
        <tr>
          <td>Mint</td>
          <td className="text-lg-right">
            <Address pubkey={info.mint} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Owner</td>
          <td className="text-lg-right">
            <Address pubkey={info.owner} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Token balance ({unit})</td>
          <td className="text-lg-right">{balance}</td>
        </tr>
        {info.state === "uninitialized" && (
          <tr>
            <td>Status</td>
            <td className="text-lg-right">Uninitialized</td>
          </tr>
        )}
        {info.rentExemptReserve && (
          <tr>
            <td>Rent-exempt reserve (SAFE)</td>
            <td className="text-lg-right">
              <>
                ◎
                <span className="text-monospace">
                  {new Intl.NumberFormat("en-US", {
                    maximumFractionDigits: 9,
                  }).format(info.rentExemptReserve.uiAmount)}
                </span>
              </>
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

function MultisigAccountCard({
  account,
  info,
}: {
  account: Account;
  info: MultisigAccountInfo;
}) {
  const refresh = useFetchAccountInfo();

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Multisig Account
        </h3>
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
          <td>Required Signers</td>
          <td className="text-lg-right">{info.numRequiredSigners}</td>
        </tr>
        <tr>
          <td>Valid Signers</td>
          <td className="text-lg-right">{info.numValidSigners}</td>
        </tr>
        {info.signers.map((signer) => (
          <tr key={signer.toString()}>
            <td>Signer</td>
            <td className="text-lg-right">
              <Address pubkey={signer} alignRight link />
            </td>
          </tr>
        ))}
        {!info.isInitialized && (
          <tr>
            <td>Status</td>
            <td className="text-lg-right">Uninitialized</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
