import { PublicKey } from "@solana/web3.js";
import { AnchorAccountCard } from "components/account/AnchorAccountCard";
import { AnchorProgramCard } from "components/account/AnchorProgramCard";
import { BlockhashesCard } from "components/account/BlockhashesCard";
import { ConfigAccountSection } from "components/account/ConfigAccountSection";
import { DomainsCard } from "components/account/DomainsCard";
import { TokenInstructionsCard } from "components/account/history/TokenInstructionsCard";
import { TokenTransfersCard } from "components/account/history/TokenTransfersCard";
import { TransactionHistoryCard } from "components/account/history/TransactionHistoryCard";
import { MetaplexMetadataCard } from "components/account/MetaplexMetadataCard";
import { MetaplexNFTAttributesCard } from "components/account/MetaplexNFTAttributesCard";
import { MetaplexNFTHeader } from "components/account/MetaplexNFTHeader";
import { NonceAccountSection } from "components/account/NonceAccountSection";
import { OwnedTokensCard } from "components/account/OwnedTokensCard";
import { RewardsCard } from "components/account/RewardsCard";
import { SecurityCard } from "components/account/SecurityCard";
import { SlotHashesCard } from "components/account/SlotHashesCard";
import { StakeAccountSection } from "components/account/StakeAccountSection";
import { StakeHistoryCard } from "components/account/StakeHistoryCard";
import { SysvarAccountSection } from "components/account/SysvarAccountSection";
import { TokenAccountSection } from "components/account/TokenAccountSection";
import { TokenHistoryCard } from "components/account/TokenHistoryCard";
import { TokenLargestAccountsCard } from "components/account/TokenLargestAccountsCard";
import { UnknownAccountCard } from "components/account/UnknownAccountCard";
import { UpgradeableLoaderAccountSection } from "components/account/UpgradeableLoaderAccountSection";
import { VoteAccountSection } from "components/account/VoteAccountSection";
import { VotesCard } from "components/account/VotesCard";
import { ErrorCard } from "components/common/ErrorCard";
import { Identicon } from "components/common/Identicon";
import { LoadingCard } from "components/common/LoadingCard";
import {
  Account,
  TokenProgramData,
  useAccountInfo,
  useFetchAccountInfo,
  useMintAccountInfo,
} from "providers/accounts";
import { useFlaggedAccounts } from "providers/accounts/flagged-accounts";
import isMetaplexNFT from "providers/accounts/utils/isMetaplexNFT";
import { useAnchorProgram } from "providers/anchor";
import { CacheEntry, FetchStatus } from "providers/cache";
import { ClusterStatus, useCluster } from "providers/cluster";
import { useTokenRegistry } from "providers/mints/token-registry";
import React from "react";
import { NavLink, Redirect, useLocation } from "react-router-dom";
import { clusterPath } from "utils/url";
import { NFTokenAccountHeader } from "../components/account/NFTokenAccountHeader";
import { NFTokenAccountSection } from "../components/account/NFTokenAccountSection";
import { NFTokenCollectionNFTGrid } from "../components/account/NFTokenCollectionNFTGrid";
import { NFTOKEN_ADDRESS } from "../utils/nftoken";
import {
  isNFTokenAccount,
  parseNFTokenCollectionAccount,
} from "../utils/nftoken-parsers.ts/isNFTokenAccount";

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
  "spl-token:mint:metaplexNFT": [
    {
      slug: "metadata",
      title: "Metadata",
      path: "/metadata",
    },
    {
      slug: "attributes",
      title: "Attributes",
      path: "/attributes",
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
  "bpf-upgradeable-loader": [
    {
      slug: "security",
      title: "Security",
      path: "/security",
    },
  ],
  "nftoken:collection": [
    {
      slug: "nftoken-collection-nfts",
      title: "NFTs",
      path: "/nfts",
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
  const { status } = useCluster();
  const info = useAccountInfo(address);
  let pubkey: PublicKey | undefined;

  try {
    pubkey = new PublicKey(address);
  } catch (err) {}

  // Fetch account on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected && pubkey) {
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
  const mintInfo = useMintAccountInfo(address);
  const account = info?.data;
  const data = account?.details?.data;
  const isToken = data?.program === "spl-token" && data?.parsed.type === "mint";

  if (isMetaplexNFT(data, mintInfo)) {
    return (
      <MetaplexNFTHeader
        nftData={(data as TokenProgramData).nftData!}
        address={address}
      />
    );
  }

  const nftokenNFT = isNFTokenAccount(account);
  if (nftokenNFT && account) {
    return <NFTokenAccountHeader account={account} />;
  }

  if (isToken) {
    let token;
    let unverified = false;

    // Fall back to legacy token list when there is stub metadata (blank uri), updatable by default by the mint authority
    if (!data?.nftData?.metadata.data.uri && tokenDetails) {
      token = tokenDetails;
    } else if (data?.nftData) {
      token = {
        logoURI: data?.nftData?.json?.image,
        name: data?.nftData?.json?.name ?? data?.nftData.metadata.data.name,
      };
      unverified = true;
    } else if (tokenDetails) {
      token = tokenDetails;
    }

    return (
      <div className="row align-items-end">
        {unverified && (
          <div className="alert alert-warning alert-scam" role="alert">
            Warning! Token names and logos are not unique. This token may have
            spoofed its name and logo to look like another token. Verify the
            token's mint address to ensure it is correct.
          </div>
        )}
        <div className="col-auto">
          <div className="avatar avatar-lg header-avatar-top">
            {token?.logoURI ? (
              <img
                src={token.logoURI}
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

        <div className="col mb-3 ms-n3 ms-md-n2">
          <h6 className="header-pretitle">Token</h6>
          <h2 className="header-title">{token?.name || "Unknown Token"}</h2>
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
  const tabComponents = getTabs(pubkey, account).concat(
    getAnchorTabs(pubkey, account)
  );

  let moreTab: MoreTabs = "history";
  if (
    tab &&
    tabComponents.filter((tabComponent) => tabComponent.tab.slug === tab)
      .length === 0
  ) {
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
      <InfoSection account={account} />
      <MoreSection
        account={account}
        tab={moreTab}
        tabs={tabComponents.map(({ component }) => component)}
      />
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
  } else if (account.details?.owner.toBase58() === NFTOKEN_ADDRESS) {
    return <NFTokenAccountSection account={account} />;
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

type TabComponent = {
  tab: Tab;
  component: JSX.Element | null;
};

export type MoreTabs =
  | "history"
  | "tokens"
  | "nftoken-collection-nfts"
  | "largest"
  | "vote-history"
  | "slot-hashes"
  | "stake-history"
  | "blockhashes"
  | "transfers"
  | "instructions"
  | "rewards"
  | "metadata"
  | "attributes"
  | "domains"
  | "security"
  | "anchor-program"
  | "anchor-account";

function MoreSection({
  account,
  tab,
  tabs,
}: {
  account: Account;
  tab: MoreTabs;
  tabs: (JSX.Element | null)[];
}) {
  const pubkey = account.pubkey;
  const data = account?.details?.data;

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body pt-0">
            <ul className="nav nav-tabs nav-overflow header-tabs">{tabs}</ul>
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
      {tab === "metadata" && (
        <MetaplexMetadataCard
          nftData={(account.details?.data as TokenProgramData).nftData!}
        />
      )}
      {tab === "nftoken-collection-nfts" && (
        <NFTokenCollectionNFTGrid collection={account.pubkey.toBase58()} />
      )}
      {tab === "attributes" && (
        <MetaplexNFTAttributesCard
          nftData={(account.details?.data as TokenProgramData).nftData!}
        />
      )}
      {tab === "domains" && <DomainsCard pubkey={pubkey} />}
      {tab === "security" && data?.program === "bpf-upgradeable-loader" && (
        <SecurityCard data={data} />
      )}
      {tab === "anchor-program" && (
        <React.Suspense
          fallback={<LoadingCard message="Loading anchor program IDL" />}
        >
          <AnchorProgramCard programId={pubkey} />
        </React.Suspense>
      )}
      {tab === "anchor-account" && (
        <React.Suspense
          fallback={
            <LoadingCard message="Decoding account data using anchor interface" />
          }
        >
          <AnchorAccountCard account={account} />
        </React.Suspense>
      )}
    </>
  );
}

function getTabs(pubkey: PublicKey, account: Account): TabComponent[] {
  const address = pubkey.toBase58();
  const data = account.details?.data;
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

  // Add the key for Metaplex NFTs
  if (
    data &&
    programTypeKey === "spl-token:mint" &&
    (data as TokenProgramData).nftData
  ) {
    tabs.push(...TABS_LOOKUP[`${programTypeKey}:metaplexNFT`]);
  }

  const isNFToken = isNFTokenAccount(account);
  if (isNFToken) {
    const collection = parseNFTokenCollectionAccount(account);
    if (collection) {
      tabs.push({
        slug: "nftoken-collection-nfts",
        title: "NFTs",
        path: "/nftoken-collection-nfts",
      });
    }
  }

  if (
    !isNFToken &&
    (!data ||
      !(
        TOKEN_TABS_HIDDEN.includes(data.program) ||
        TOKEN_TABS_HIDDEN.includes(programTypeKey)
      ))
  ) {
    tabs.push({
      slug: "tokens",
      title: "Tokens",
      path: "/tokens",
    });
    tabs.push({
      slug: "domains",
      title: "Domains",
      path: "/domains",
    });
  }

  return tabs.map((tab) => {
    return {
      tab,
      component: (
        <li key={tab.slug} className="nav-item">
          <NavLink
            className="nav-link"
            to={clusterPath(`/address/${address}${tab.path}`)}
            exact
          >
            {tab.title}
          </NavLink>
        </li>
      ),
    };
  });
}

function getAnchorTabs(pubkey: PublicKey, account: Account) {
  const tabComponents = [];
  const anchorProgramTab: Tab = {
    slug: "anchor-program",
    title: "Anchor Program IDL",
    path: "/anchor-program",
  };
  tabComponents.push({
    tab: anchorProgramTab,
    component: (
      <React.Suspense key={anchorProgramTab.slug} fallback={<></>}>
        <AnchorProgramLink
          tab={anchorProgramTab}
          address={pubkey.toString()}
          pubkey={pubkey}
        />
      </React.Suspense>
    ),
  });

  const accountDataTab: Tab = {
    slug: "anchor-account",
    title: "Anchor Data",
    path: "/anchor-account",
  };
  tabComponents.push({
    tab: accountDataTab,
    component: (
      <React.Suspense key={accountDataTab.slug} fallback={<></>}>
        <AccountDataLink
          tab={accountDataTab}
          address={pubkey.toString()}
          programId={account.details?.owner}
        />
      </React.Suspense>
    ),
  });

  return tabComponents;
}

function AnchorProgramLink({
  tab,
  address,
  pubkey,
}: {
  tab: Tab;
  address: string;
  pubkey: PublicKey;
}) {
  const { url } = useCluster();
  const anchorProgram = useAnchorProgram(pubkey.toString() ?? "", url);

  if (!anchorProgram) {
    return null;
  }

  return (
    <li key={tab.slug} className="nav-item">
      <NavLink
        className="nav-link"
        to={clusterPath(`/address/${address}${tab.path}`)}
        exact
      >
        {tab.title}
      </NavLink>
    </li>
  );
}

function AccountDataLink({
  address,
  tab,
  programId,
}: {
  address: string;
  tab: Tab;
  programId: PublicKey | undefined;
}) {
  const { url } = useCluster();
  const accountAnchorProgram = useAnchorProgram(
    programId?.toString() ?? "",
    url
  );

  if (!accountAnchorProgram) {
    return null;
  }

  return (
    <li key={tab.slug} className="nav-item">
      <NavLink
        className="nav-link"
        to={clusterPath(`/address/${address}${tab.path}`)}
        exact
      >
        {tab.title}
      </NavLink>
    </li>
  );
}
