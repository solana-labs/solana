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
import { useFetchTokenSupply, useTokenSupply } from "providers/mints/supply";
import { FetchStatus } from "providers/cache";

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
  } catch (err) {}
  return <UnknownAccountCard account={account} />;
}

function MintAccountCard({
  account,
  info,
}: {
  account: Account;
  info: MintAccountInfo;
}) {
  const mintAddress = account.pubkey.toBase58();
  const fetchInfo = useFetchAccountInfo();
  const supply = useTokenSupply(mintAddress);
  const fetchSupply = useFetchTokenSupply();
  const refreshSupply = () => fetchSupply(account.pubkey);
  const refresh = () => {
    fetchInfo(account.pubkey);
    refreshSupply();
  };

  let renderSupply;
  const supplyTotal = supply?.data?.uiAmount;
  if (supplyTotal === undefined) {
    if (!supply || supply?.status === FetchStatus.Fetching) {
      renderSupply = (
        <>
          <span className="spinner-grow spinner-grow-sm mr-2"></span>
          Loading
        </>
      );
    } else {
      renderSupply = "Fetch failed";
    }
  } else {
    renderSupply = supplyTotal;
  }

  React.useEffect(() => {
    if (!supply) refreshSupply();
  }, [mintAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Token Mint Account
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
          <td>Total Supply</td>
          <td className="text-lg-right">{renderSupply}</td>
        </tr>
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
        {info.owner !== undefined && (
          <tr>
            <td>Owner</td>
            <td className="text-lg-right">
              <Address pubkey={info.owner} alignRight link />
            </td>
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

  let balance;
  if ("amount" in info) {
    balance = info.amount;
  } else {
    balance = info.tokenAmount?.uiAmount;
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
          <td>Balance (tokens)</td>
          <td className="text-lg-right">{balance}</td>
        </tr>
        <tr>
          <td>Status</td>
          <td className="text-lg-right">
            {info.isInitialized ? "Initialized" : "Uninitialized"}
          </td>
        </tr>
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
        <tr>
          <td>Status</td>
          <td className="text-lg-right">
            {info.isInitialized ? "Initialized" : "Uninitialized"}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}
