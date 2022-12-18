import {
  Account,
  NFTData,
  TokenProgramData,
  useFetchAccountInfo,
} from "providers/accounts";
import {
  TokenAccount,
  MintAccountInfo,
  TokenAccountInfo,
  MultisigAccountInfo,
} from "validators/accounts/token";
import { create } from "superstruct";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { UnknownAccountCard } from "./UnknownAccountCard";
import { Cluster, useCluster } from "providers/cluster";
import { abbreviatedNumber, normalizeTokenAmount } from "utils";
import { addressLabel } from "utils/tx";
import { reportError } from "utils/sentry";
import { useTokenRegistry } from "providers/mints/token-registry";
import { BigNumber } from "bignumber.js";
import { Copyable } from "components/common/Copyable";
import { CoingeckoStatus, useCoinGecko } from "utils/coingecko";
import { displayTimestampWithoutDate } from "utils/date";
import { LoadingCard } from "components/common/LoadingCard";
import { PublicKey } from "@solana/web3.js";
import isMetaplexNFT from "providers/accounts/utils/isMetaplexNFT";

const getEthAddress = (link?: string) => {
  let address = "";
  if (link) {
    const extractEth = link.match(/0x[a-fA-F0-9]{40,64}/);

    if (extractEth) {
      address = extractEth[0];
    }
  }

  return address;
};

export function TokenAccountSection({
  account,
  tokenAccount,
}: {
  account: Account;
  tokenAccount: TokenAccount;
}) {
  const { cluster } = useCluster();

  try {
    switch (tokenAccount.type) {
      case "mint": {
        const info = create(tokenAccount.info, MintAccountInfo);

        if (isMetaplexNFT(account.data.parsed, info)) {
          return (
            <NonFungibleTokenMintAccountCard
              account={account}
              nftData={(account.data.parsed as TokenProgramData).nftData!}
              mintInfo={info}
            />
          );
        }

        return <FungibleTokenMintAccountCard account={account} info={info} />;
      }
      case "account": {
        const info = create(tokenAccount.info, TokenAccountInfo);
        return <TokenAccountCard account={account} info={info} />;
      }
      case "multisig": {
        const info = create(tokenAccount.info, MultisigAccountInfo);
        return <MultisigAccountCard account={account} info={info} />;
      }
    }
  } catch (err) {
    if (cluster !== Cluster.Custom) {
      reportError(err, {
        address: account.pubkey.toBase58(),
      });
    }
  }
  return <UnknownAccountCard account={account} />;
}

function FungibleTokenMintAccountCard({
  account,
  info,
}: {
  account: Account;
  info: MintAccountInfo;
}) {
  const { tokenRegistry } = useTokenRegistry();
  const mintAddress = account.pubkey.toBase58();
  const fetchInfo = useFetchAccountInfo();
  const refresh = () => fetchInfo(account.pubkey, "parsed");
  const tokenInfo = tokenRegistry.get(mintAddress);

  const bridgeContractAddress = getEthAddress(
    tokenInfo?.extensions?.bridgeContract
  );
  const assetContractAddress = getEthAddress(
    tokenInfo?.extensions?.assetContract
  );

  const coinInfo = useCoinGecko(tokenInfo?.extensions?.coingeckoId);

  let tokenPriceInfo;
  let tokenPriceDecimals = 2;
  if (coinInfo?.status === CoingeckoStatus.Success) {
    tokenPriceInfo = coinInfo.coinInfo;
    if (tokenPriceInfo && tokenPriceInfo.price < 1) {
      tokenPriceDecimals = 6;
    }
  }

  return (
    <>
      {tokenInfo?.extensions?.coingeckoId &&
        coinInfo?.status === CoingeckoStatus.Loading && (
          <LoadingCard message="Loading token price data" />
        )}
      {tokenPriceInfo && tokenPriceInfo.price && (
        <div className="row">
          <div className="col-12 col-lg-4 col-xl">
            <div className="card">
              <div className="card-body">
                <h4>
                  Price{" "}
                  {tokenPriceInfo.market_cap_rank && (
                    <span className="ms-2 badge bg-primary rank">
                      Rank #{tokenPriceInfo.market_cap_rank}
                    </span>
                  )}
                </h4>
                <h1 className="mb-0">
                  ${tokenPriceInfo.price.toFixed(tokenPriceDecimals)}{" "}
                  {tokenPriceInfo.price_change_percentage_24h > 0 && (
                    <small className="change-positive">
                      &uarr;{" "}
                      {tokenPriceInfo.price_change_percentage_24h.toFixed(2)}%
                    </small>
                  )}
                  {tokenPriceInfo.price_change_percentage_24h < 0 && (
                    <small className="change-negative">
                      &darr;{" "}
                      {tokenPriceInfo.price_change_percentage_24h.toFixed(2)}%
                    </small>
                  )}
                  {tokenPriceInfo.price_change_percentage_24h === 0 && (
                    <small>0%</small>
                  )}
                </h1>
              </div>
            </div>
          </div>
          <div className="col-12 col-lg-4 col-xl">
            <div className="card">
              <div className="card-body">
                <h4>24 Hour Volume</h4>
                <h1 className="mb-0">
                  ${abbreviatedNumber(tokenPriceInfo.volume_24)}
                </h1>
              </div>
            </div>
          </div>
          <div className="col-12 col-lg-4 col-xl">
            <div className="card">
              <div className="card-body">
                <h4>Market Cap</h4>
                <h1 className="mb-0">
                  ${abbreviatedNumber(tokenPriceInfo.market_cap)}
                </h1>
                <p className="updated-time text-muted">
                  Updated at{" "}
                  {displayTimestampWithoutDate(
                    tokenPriceInfo.last_updated.getTime()
                  )}
                </p>
              </div>
            </div>
          </div>
        </div>
      )}
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            {tokenInfo ? "Overview" : "Token Mint"}
          </h3>
          <button className="btn btn-white btn-sm" onClick={refresh}>
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
            <td>
              {info.mintAuthority === null ? "Fixed Supply" : "Current Supply"}
            </td>
            <td className="text-lg-end">
              {normalizeTokenAmount(info.supply, info.decimals).toLocaleString(
                "en-US",
                {
                  maximumFractionDigits: 20,
                }
              )}
            </td>
          </tr>
          {tokenInfo?.extensions?.website && (
            <tr>
              <td>Website</td>
              <td className="text-lg-end">
                <a
                  rel="noopener noreferrer"
                  target="_blank"
                  href={tokenInfo.extensions.website}
                >
                  {tokenInfo.extensions.website}
                  <span className="fe fe-external-link ms-2"></span>
                </a>
              </td>
            </tr>
          )}
          {info.mintAuthority && (
            <tr>
              <td>Mint Authority</td>
              <td className="text-lg-end">
                <Address pubkey={info.mintAuthority} alignRight link />
              </td>
            </tr>
          )}
          {info.freezeAuthority && (
            <tr>
              <td>Freeze Authority</td>
              <td className="text-lg-end">
                <Address pubkey={info.freezeAuthority} alignRight link />
              </td>
            </tr>
          )}
          <tr>
            <td>Decimals</td>
            <td className="text-lg-end">{info.decimals}</td>
          </tr>
          {!info.isInitialized && (
            <tr>
              <td>Status</td>
              <td className="text-lg-end">Uninitialized</td>
            </tr>
          )}
          {tokenInfo?.extensions?.bridgeContract && bridgeContractAddress && (
            <tr>
              <td>Bridge Contract</td>
              <td className="text-lg-end">
                <Copyable text={bridgeContractAddress}>
                  <a
                    href={tokenInfo.extensions.bridgeContract}
                    target="_blank"
                    rel="noreferrer"
                  >
                    {bridgeContractAddress}
                  </a>
                </Copyable>
              </td>
            </tr>
          )}
          {tokenInfo?.extensions?.assetContract && assetContractAddress && (
            <tr>
              <td>Bridged Asset Contract</td>
              <td className="text-lg-end">
                <Copyable text={assetContractAddress}>
                  <a
                    href={tokenInfo.extensions.bridgeContract}
                    target="_blank"
                    rel="noreferrer"
                  >
                    {assetContractAddress}
                  </a>
                </Copyable>
              </td>
            </tr>
          )}
        </TableCardBody>
      </div>
    </>
  );
}

function NonFungibleTokenMintAccountCard({
  account,
  nftData,
  mintInfo,
}: {
  account: Account;
  nftData: NFTData;
  mintInfo: MintAccountInfo;
}) {
  const fetchInfo = useFetchAccountInfo();
  const refresh = () => fetchInfo(account.pubkey, "parsed");

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Overview
        </h3>
        <button className="btn btn-white btn-sm" onClick={refresh}>
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
        {nftData.editionInfo.masterEdition?.maxSupply && (
          <tr>
            <td>Max Total Supply</td>
            <td className="text-lg-end">
              {nftData.editionInfo.masterEdition.maxSupply.toNumber() === 0
                ? 1
                : nftData.editionInfo.masterEdition.maxSupply.toNumber()}
            </td>
          </tr>
        )}
        {nftData?.editionInfo.masterEdition?.supply && (
          <tr>
            <td>Current Supply</td>
            <td className="text-lg-end">
              {nftData.editionInfo.masterEdition.supply.toNumber() === 0
                ? 1
                : nftData.editionInfo.masterEdition.supply.toNumber()}
            </td>
          </tr>
        )}
        {!!nftData?.metadata.collection?.verified && (
          <tr>
            <td>Verified Collection Address</td>
            <td className="text-lg-end">
              <Address
                pubkey={new PublicKey(nftData.metadata.collection.key)}
                alignRight
                link
              />
            </td>
          </tr>
        )}
        {mintInfo.mintAuthority && (
          <tr>
            <td>Mint Authority</td>
            <td className="text-lg-end">
              <Address pubkey={mintInfo.mintAuthority} alignRight link />
            </td>
          </tr>
        )}
        {mintInfo.freezeAuthority && (
          <tr>
            <td>Freeze Authority</td>
            <td className="text-lg-end">
              <Address pubkey={mintInfo.freezeAuthority} alignRight link />
            </td>
          </tr>
        )}
        <tr>
          <td>Update Authority</td>
          <td className="text-lg-end">
            <Address
              pubkey={new PublicKey(nftData.metadata.updateAuthority)}
              alignRight
              link
            />
          </td>
        </tr>
        {nftData?.json && nftData.json.external_url && (
          <tr>
            <td>Website</td>
            <td className="text-lg-end">
              <a
                rel="noopener noreferrer"
                target="_blank"
                href={nftData.json.external_url}
              >
                {nftData.json.external_url}
                <span className="fe fe-external-link ms-2"></span>
              </a>
            </td>
          </tr>
        )}
        {nftData?.metadata.data && (
          <tr>
            <td>Seller Fee</td>
            <td className="text-lg-end">
              {`${nftData?.metadata.data.sellerFeeBasisPoints / 100}%`}
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
  const { cluster } = useCluster();
  const { tokenRegistry } = useTokenRegistry();
  const label = addressLabel(account.pubkey.toBase58(), cluster, tokenRegistry);

  let unit, balance;
  if (info.isNative) {
    unit = "SOL";
    balance = (
      <>
        ◎
        <span className="font-monospace">
          {new BigNumber(info.tokenAmount.uiAmountString).toFormat(9)}
        </span>
      </>
    );
  } else {
    balance = <>{info.tokenAmount.uiAmountString}</>;
    unit = tokenRegistry.get(info.mint.toBase58())?.symbol || "tokens";
  }

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Token Account
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
        {label && (
          <tr>
            <td>Address Label</td>
            <td className="text-lg-end">{label}</td>
          </tr>
        )}
        <tr>
          <td>Mint</td>
          <td className="text-lg-end">
            <Address pubkey={info.mint} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Owner</td>
          <td className="text-lg-end">
            <Address pubkey={info.owner} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Token balance ({unit})</td>
          <td className="text-lg-end">{balance}</td>
        </tr>
        {info.state === "uninitialized" && (
          <tr>
            <td>Status</td>
            <td className="text-lg-end">Uninitialized</td>
          </tr>
        )}
        {info.rentExemptReserve && (
          <tr>
            <td>Rent-exempt reserve (SOL)</td>
            <td className="text-lg-end">
              <>
                ◎
                <span className="font-monospace">
                  {new BigNumber(
                    info.rentExemptReserve.uiAmountString
                  ).toFormat(9)}
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
          <td>Required Signers</td>
          <td className="text-lg-end">{info.numRequiredSigners}</td>
        </tr>
        <tr>
          <td>Valid Signers</td>
          <td className="text-lg-end">{info.numValidSigners}</td>
        </tr>
        {info.signers.map((signer) => (
          <tr key={signer.toString()}>
            <td>Signer</td>
            <td className="text-lg-end">
              <Address pubkey={signer} alignRight link />
            </td>
          </tr>
        ))}
        {!info.isInitialized && (
          <tr>
            <td>Status</td>
            <td className="text-lg-end">Uninitialized</td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}
