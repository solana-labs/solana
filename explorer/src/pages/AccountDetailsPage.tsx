import React from "react";
import "bootstrap/dist/js/bootstrap.min.js";
import { PublicKey } from "@solana/web3.js";
import { CacheEntry, FetchStatus } from "providers/cache";
import {
  useFetchAccountInfo,
  useAccountInfo,
  Account,
  ProgramData,
} from "providers/accounts";
import { StakeAccountSection } from "components/account/StakeAccountSection";
import { TokenAccountSection } from "components/account/TokenAccountSection";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { useCluster, ClusterStatus } from "providers/cluster";
import { NavLink, Redirect, useLocation, Link } from "react-router-dom";
import { clusterPath } from "utils/url";
import { UnknownAccountCard } from "components/account/UnknownAccountCard";
import { OwnedTokensCard } from "components/account/OwnedTokensCard";
import { TokenHistoryCard } from "components/account/TokenHistoryCard";
import { TokenLargestAccountsCard } from "components/account/TokenLargestAccountsCard";
import { VoteAccountSection } from "components/account/VoteAccountSection";
import { NonceAccountSection } from "components/account/NonceAccountSection";
import { VotesCard } from "components/account/VotesCard";
import { SysvarAccountSection } from "components/account/SysvarAccountSection";
import { SlotHashesCard } from "components/account/SlotHashesCard";
import { StakeHistoryCard } from "components/account/StakeHistoryCard";
import { BlockhashesCard } from "components/account/BlockhashesCard";
import { ConfigAccountSection } from "components/account/ConfigAccountSection";
import { useFlaggedAccounts } from "providers/accounts/flagged-accounts";
import { UpgradeableLoaderAccountSection } from "components/account/UpgradeableLoaderAccountSection";
import { useTokenRegistry } from "providers/mints/token-registry";
import { Identicon } from "components/common/Identicon";
import { TransactionHistoryCard } from "components/account/history/TransactionHistoryCard";
import { TokenTransfersCard } from "components/account/history/TokenTransfersCard";
import { TokenInstructionsCard } from "components/account/history/TokenInstructionsCard";
import { RewardsCard } from "components/account/RewardsCard";
import { Creator } from "metaplex/classes";
import { ArtContent } from "metaplex/Art/Art";
import { AccountMetadata, useFetchMetadata, useMetadata } from "providers/accounts/metadata";

const IDENTICON_WIDTH = 64;

const TABS_LOOKUP: { [id: string]: Tab[] } = {
  "spl-token:mint": [
    {
      slug: "transfers",
      title: "Transfers",
      path: "/transfers",
    },
    {
      slug: "instructions",
      title: "Instructions",
      path: "/instructions",
    },
    {
      slug: "largest",
      title: "Distribution",
      path: "/largest",
    },
  ],
  stake: [
    {
      slug: "rewards",
      title: "Rewards",
      path: "/rewards",
    },
  ],
  vote: [
    {
      slug: "vote-history",
      title: "Vote History",
      path: "/vote-history",
    },
    {
      slug: "rewards",
      title: "Rewards",
      path: "/rewards",
    },
  ],
  "sysvar:recentBlockhashes": [
    {
      slug: "blockhashes",
      title: "Blockhashes",
      path: "/blockhashes",
    },
  ],
  "sysvar:slotHashes": [
    {
      slug: "slot-hashes",
      title: "Slot Hashes",
      path: "/slot-hashes",
    },
  ],
  "sysvar:stakeHistory": [
    {
      slug: "stake-history",
      title: "Stake History",
      path: "/stake-history",
    },
  ],
};

const TOKEN_TABS_HIDDEN = [
  "spl-token:mint",
  "config",
  "vote",
  "sysvar",
  "config",
];

type Props = { address: string; tab?: string };
export function AccountDetailsPage({ address, tab }: Props) {
  const fetchAccount = useFetchAccountInfo();
  const fetchMetadata = useFetchMetadata();
  const { status } = useCluster();
  const info = useAccountInfo(address);
  let pubkey: PublicKey | undefined;

  try {
    pubkey = new PublicKey(address);
  } catch (err) { }

  // Fetch account and metadata on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected && pubkey) {
      fetchMetadata(pubkey);
      fetchAccount(pubkey);
    }
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <AccountHeader address={address} info={info} />
        </div>
      </div>
      {!pubkey ? (
        <ErrorCard text={`Address "${address}" is not valid`} />
      ) : (
        <DetailsSections pubkey={pubkey} tab={tab} info={info} />
      )}
    </div>
  );
}

export function AccountHeader({
  address,
  info,
}: {
  address: string;
  info?: CacheEntry<Account>;
}) {
  const { tokenRegistry } = useTokenRegistry();
  const tokenDetails = tokenRegistry.get(address);
  const account = info?.data;
  const data = account?.details?.data;
  const isToken = data?.program === "spl-token" && data?.parsed.type === "mint";
  const metadata = useMetadata(address);
  const fetchingMetadata = metadata?.status === FetchStatus.Fetching;
  const isNFT = isToken && metadata?.data?.meta;

  if (isNFT) {
    return <NFTHeader metadata={metadata} address={address} />
  }

  if (!fetchingMetadata && (tokenDetails || isToken)) {
    return (
      <div className="row align-items-end">
        <div className="col-auto">
          <div className="avatar avatar-lg header-avatar-top">
            {tokenDetails?.logoURI ? (
              <img
                src={tokenDetails.logoURI}
                alt="token logo"
                className="avatar-img rounded-circle border border-4 border-body"
              />
            ) : (
              <Identicon
                address={address}
                className="avatar-img rounded-circle border border-body identicon-wrapper"
                style={{ width: IDENTICON_WIDTH }}
              />
            )}
          </div>
        </div>

        <div className="col mb-3 ml-n3 ml-md-n2">
          <h6 className="header-pretitle">Token</h6>
          <h2 className="header-title">
            {tokenDetails?.name || "Unknown Token"}
          </h2>
        </div>
      </div>
    );
  }

  return (
    <>
      <h6 className="header-pretitle">Details</h6>
      <h2 className="header-title">Account</h2>
    </>
  );
}

export function NFTHeader({
  metadata,
  address,
}: {
  metadata: CacheEntry<AccountMetadata>;
  address: string;
}) {

  const meta = metadata.data?.meta!;
  return (
    <div className="row align-items-begin">
      <div className="col-auto ml-2">
        <ArtContent metadata={meta} pubkey={address} preview={false} />
      </div>

      <div className="col mb-3 ml-n3 ml-md-n2 mt-3">
        <h6 className="header-pretitle">NFT</h6>
        <h2 className="header-title">
          {meta.data?.name}
        </h2>
        <h4 className="header-pretitle mt-1"> {meta.data?.symbol}</h4>
        <div className="mb-3 mt-2">
          <span className="badge badge-pill badge-dark mr-2">{`${meta.primarySaleHappened ? "Secondary Market" : "Primary Market"}`}</span>
          <span className="badge badge-pill badge-dark mr-2">{`${meta.isMutable ? "Mutable" : "Immutable"}`}</span>
        </div>
        <div className="btn-group">
          <button className="btn btn-dark btn-sm dropdown-toggle creators-dropdown-button-width" type="button" data-toggle="dropdown" aria-haspopup="true" aria-expanded="false">
            Creators
          </button>
          <div className="dropdown-menu">
            {getCreatorDropdownItems(meta.data.creators!)}
          </div>
        </div>
      </div>
    </div >
  );
}


function DetailsSections({
  pubkey,
  tab,
  info,
}: {
  pubkey: PublicKey;
  tab?: string;
  info?: CacheEntry<Account>;
}) {
  const fetchAccount = useFetchAccountInfo();
  const address = pubkey.toBase58();
  const location = useLocation();
  const { flaggedAccounts } = useFlaggedAccounts();

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.data?.lamports === undefined
  ) {
    return <ErrorCard retry={() => fetchAccount(pubkey)} text="Fetch Failed" />;
  }

  const account = info.data;
  const data = account?.details?.data;
  const tabs = getTabs(data);

  let moreTab: MoreTabs = "history";
  if (tab && tabs.filter(({ slug }) => slug === tab).length === 0) {
    return <Redirect to={{ ...location, pathname: `/address/${address}` }} />;
  } else if (tab) {
    moreTab = tab as MoreTabs;
  }

  return (
    <>
      {flaggedAccounts.has(address) && (
        <div className="alert alert-danger alert-scam" role="alert">
          Warning! This account has been flagged by the community as a scam
          account. Please be cautious sending SOL to this account.
        </div>
      )}
      {<InfoSection account={account} />}
      {<MoreSection account={account} tab={moreTab} tabs={tabs} />}
    </>
  );
}

function InfoSection({ account }: { account: Account }) {
  const data = account?.details?.data;

  if (data && data.program === "bpf-upgradeable-loader") {
    return (
      <UpgradeableLoaderAccountSection
        account={account}
        parsedData={data.parsed}
        programData={data.programData}
      />
    );
  } else if (data && data.program === "stake") {
    return (
      <StakeAccountSection
        account={account}
        stakeAccount={data.parsed.info}
        activation={data.activation}
        stakeAccountType={data.parsed.type}
      />
    );
  } else if (data && data.program === "spl-token") {
    return <TokenAccountSection account={account} tokenAccount={data.parsed} />;
  } else if (data && data.program === "nonce") {
    return <NonceAccountSection account={account} nonceAccount={data.parsed} />;
  } else if (data && data.program === "vote") {
    return <VoteAccountSection account={account} voteAccount={data.parsed} />;
  } else if (data && data.program === "sysvar") {
    return (
      <SysvarAccountSection account={account} sysvarAccount={data.parsed} />
    );
  } else if (data && data.program === "config") {
    return (
      <ConfigAccountSection account={account} configAccount={data.parsed} />
    );
  } else {
    return <UnknownAccountCard account={account} />;
  }
}

type Tab = {
  slug: MoreTabs;
  title: string;
  path: string;
};

export type MoreTabs =
  | "history"
  | "tokens"
  | "largest"
  | "vote-history"
  | "slot-hashes"
  | "stake-history"
  | "blockhashes"
  | "transfers"
  | "instructions"
  | "rewards";

function MoreSection({
  account,
  tab,
  tabs,
}: {
  account: Account;
  tab: MoreTabs;
  tabs: Tab[];
}) {
  const pubkey = account.pubkey;
  const address = account.pubkey.toBase58();
  const data = account?.details?.data;
  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">
              {tabs.map(({ title, slug, path }) => (
                <li key={slug} className="nav-item">
                  <NavLink
                    className="nav-link"
                    to={clusterPath(`/address/${address}${path}`)}
                    exact
                  >
                    {title}
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
      {tab === "tokens" && (
        <>
          <OwnedTokensCard pubkey={pubkey} />
          <TokenHistoryCard pubkey={pubkey} />
        </>
      )}
      {tab === "history" && <TransactionHistoryCard pubkey={pubkey} />}
      {tab === "transfers" && <TokenTransfersCard pubkey={pubkey} />}
      {tab === "instructions" && <TokenInstructionsCard pubkey={pubkey} />}
      {tab === "largest" && <TokenLargestAccountsCard pubkey={pubkey} />}
      {tab === "rewards" && <RewardsCard pubkey={pubkey} />}
      {tab === "vote-history" && data?.program === "vote" && (
        <VotesCard voteAccount={data.parsed} />
      )}
      {tab === "slot-hashes" &&
        data?.program === "sysvar" &&
        data.parsed.type === "slotHashes" && (
          <SlotHashesCard sysvarAccount={data.parsed} />
        )}
      {tab === "stake-history" &&
        data?.program === "sysvar" &&
        data.parsed.type === "stakeHistory" && (
          <StakeHistoryCard sysvarAccount={data.parsed} />
        )}
      {tab === "blockhashes" &&
        data?.program === "sysvar" &&
        data.parsed.type === "recentBlockhashes" && (
          <BlockhashesCard blockhashes={data.parsed.info} />
        )}
    </>
  );
}

function getTabs(data?: ProgramData): Tab[] {
  const tabs: Tab[] = [
    {
      slug: "history",
      title: "History",
      path: "",
    },
  ];

  let programTypeKey = "";
  if (data && "parsed" in data && "type" in data.parsed) {
    programTypeKey = `${data.program}:${data.parsed.type}`;
  }

  if (data && data.program in TABS_LOOKUP) {
    tabs.push(...TABS_LOOKUP[data.program]);
  }

  if (data && programTypeKey in TABS_LOOKUP) {
    tabs.push(...TABS_LOOKUP[programTypeKey]);
  }

  if (
    !data ||
    !(
      TOKEN_TABS_HIDDEN.includes(data.program) ||
      TOKEN_TABS_HIDDEN.includes(programTypeKey)
    )
  ) {
    tabs.push({
      slug: "tokens",
      title: "Tokens",
      path: "/tokens",
    });
  }

  return tabs;
}

function getCreatorDropdownItems(creators?: Creator[]) {
  const getVerifiedIcon = (isVerified: boolean) => {
    const className = isVerified ? "fe fe-check-circle" : "fe fe-alert-octagon";
    return < i className={`ml-3 ${className}`} ></i >;
  }

  const CreatorEntry = (creator: Creator) => {
    return (
      <div
        className={"d-flex align-items-center creator-dropdown-entry"}
      >
        {getVerifiedIcon(creator.verified)}
        <Link
          className="dropdown-item text-monospace creator-dropdown-entry-address"
          to={`/address/${creator.address}`}
        >
          {creator.address}
        </Link>
        <div className="mr-3"> {`${creator.share}%`}</div>
      </div>
    );
  }

  let listOfCreators: JSX.Element[] = [];

  if (creators && creators?.length > 0) {
    creators?.forEach((creator) => {
      listOfCreators.push(<CreatorEntry key={creator.address} {...creator} />);
    })
  }

  return listOfCreators;
}
