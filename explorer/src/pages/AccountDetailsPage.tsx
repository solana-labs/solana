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
import FLAGGED_ACCOUNTS_WARNING from "providers/accounts/flagged-accounts";
import isMetaplexNFT from "providers/accounts/utils/isMetaplexNFT";
import { useAnchorProgram } from "providers/anchor";
import { CacheEntry, FetchStatus } from "providers/cache";
import { ClusterStatus, useCluster } from "providers/cluster";
import { useTokenRegistry } from "providers/mints/token-registry";
import React, { Suspense } from "react";
import { NavLink, Redirect, useLocation } from "react-router-dom";
import { clusterPath } from "utils/url";
import { NFTokenAccountHeader } from "../components/account/nftoken/NFTokenAccountHeader";
import { NFTokenAccountSection } from "../components/account/nftoken/NFTokenAccountSection";
import { NFTokenCollectionNFTGrid } from "../components/account/nftoken/NFTokenCollectionNFTGrid";
import { NFTOKEN_ADDRESS } from "../components/account/nftoken/nftoken";
import {
  isNFTokenAccount,
  parseNFTokenCollectionAccount,
} from "../components/account/nftoken/isNFTokenAccount";
import { isAddressLookupTableAccount } from "components/account/address-lookup-table/types";
import { AddressLookupTableAccountSection } from "components/account/address-lookup-table/AddressLookupTableAccountSection";
import { LookupTableEntriesCard } from "components/account/address-lookup-table/LookupTableEntriesCard";

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
  "address-lookup-table": [
    {
      slug: "entries",
      title: "Table Entries",
      path: "/entries",
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
      fetchAccount(pubkey, "parsed");
    }
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <AccountHeader address={address} account={info?.data} />
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
  account,
}: {
  address: string;
  account?: Account;
}) {
  const { tokenRegistry } = useTokenRegistry();
  const tokenDetails = tokenRegistry.get(address);
  const mintInfo = useMintAccountInfo(address);
  const parsedData = account?.data.parsed;
  const isToken =
    parsedData?.program === "spl-token" && parsedData?.parsed.type === "mint";

  if (isMetaplexNFT(parsedData, mintInfo)) {
    return (
      <MetaplexNFTHeader
        nftData={(parsedData as TokenProgramData).nftData!}
        address={address}
      />
    );
  }

  const nftokenNFT = account && isNFTokenAccount(account);
  if (nftokenNFT && account) {
    return <NFTokenAccountHeader account={account} />;
  }

  if (isToken) {
    let token;
    let unverified = false;

    // Fall back to legacy token list when there is stub metadata (blank uri), updatable by default by the mint authority
    if (!parsedData?.nftData?.metadata.data.uri && tokenDetails) {
      token = tokenDetails;
    } else if (parsedData?.nftData) {
      token = {
        logoURI: parsedData?.nftData?.json?.image,
        name:
          parsedData?.nftData?.json?.name ??
          parsedData?.nftData.metadata.data.name,
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

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.data?.lamports === undefined
  ) {
    return (
      <ErrorCard
        retry={() => fetchAccount(pubkey, "parsed")}
        text="Fetch Failed"
      />
    );
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
      {FLAGGED_ACCOUNTS_WARNING[address] ?? null}
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
  const parsedData = account.data.parsed;
  const rawData = account.data.raw;

  if (parsedData && parsedData.program === "bpf-upgradeable-loader") {
    return (
      <UpgradeableLoaderAccountSection
        account={account}
        parsedData={parsedData.parsed}
        programData={parsedData.programData}
      />
    );
  } else if (parsedData && parsedData.program === "stake") {
    return (
      <StakeAccountSection
        account={account}
        stakeAccount={parsedData.parsed.info}
        activation={parsedData.activation}
        stakeAccountType={parsedData.parsed.type}
      />
    );
  } else if (account.owner.toBase58() === NFTOKEN_ADDRESS) {
    return <NFTokenAccountSection account={account} />;
  } else if (parsedData && parsedData.program === "spl-token") {
    return (
      <TokenAccountSection account={account} tokenAccount={parsedData.parsed} />
    );
  } else if (parsedData && parsedData.program === "nonce") {
    return (
      <NonceAccountSection account={account} nonceAccount={parsedData.parsed} />
    );
  } else if (parsedData && parsedData.program === "vote") {
    return (
      <VoteAccountSection account={account} voteAccount={parsedData.parsed} />
    );
  } else if (parsedData && parsedData.program === "sysvar") {
    return (
      <SysvarAccountSection
        account={account}
        sysvarAccount={parsedData.parsed}
      />
    );
  } else if (parsedData && parsedData.program === "config") {
    return (
      <ConfigAccountSection
        account={account}
        configAccount={parsedData.parsed}
      />
    );
  } else if (
    parsedData &&
    parsedData.program === "address-lookup-table" &&
    parsedData.parsed.type === "lookupTable"
  ) {
    return (
      <AddressLookupTableAccountSection
        account={account}
        lookupTableAccount={parsedData.parsed.info}
      />
    );
  } else if (rawData && isAddressLookupTableAccount(account.owner, rawData)) {
    return (
      <AddressLookupTableAccountSection account={account} data={rawData} />
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
  | "anchor-account"
  | "entries";

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
  const parsedData = account.data.parsed;
  const rawData = account.data.raw;

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
      {tab === "vote-history" && parsedData?.program === "vote" && (
        <VotesCard voteAccount={parsedData.parsed} />
      )}
      {tab === "slot-hashes" &&
        parsedData?.program === "sysvar" &&
        parsedData.parsed.type === "slotHashes" && (
          <SlotHashesCard sysvarAccount={parsedData.parsed} />
        )}
      {tab === "stake-history" &&
        parsedData?.program === "sysvar" &&
        parsedData.parsed.type === "stakeHistory" && (
          <StakeHistoryCard sysvarAccount={parsedData.parsed} />
        )}
      {tab === "blockhashes" &&
        parsedData?.program === "sysvar" &&
        parsedData.parsed.type === "recentBlockhashes" && (
          <BlockhashesCard blockhashes={parsedData.parsed.info} />
        )}
      {tab === "metadata" && (
        <MetaplexMetadataCard
          nftData={(account.data.parsed as TokenProgramData).nftData!}
        />
      )}
      {tab === "nftoken-collection-nfts" && (
        <Suspense
          fallback={<LoadingCard message="Loading NFTs for collection." />}
        >
          <NFTokenCollectionNFTGrid collection={account.pubkey.toBase58()} />
        </Suspense>
      )}
      {tab === "attributes" && (
        <MetaplexNFTAttributesCard
          nftData={(account.data.parsed as TokenProgramData).nftData!}
        />
      )}
      {tab === "domains" && <DomainsCard pubkey={pubkey} />}
      {tab === "security" &&
        parsedData?.program === "bpf-upgradeable-loader" && (
          <SecurityCard data={parsedData} />
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
      {tab === "entries" &&
        rawData &&
        isAddressLookupTableAccount(account.owner, rawData) && (
          <LookupTableEntriesCard lookupTableAccountData={rawData} />
        )}
      {tab === "entries" &&
        parsedData?.program === "address-lookup-table" &&
        parsedData.parsed.type === "lookupTable" && (
          <LookupTableEntriesCard parsedLookupTable={parsedData.parsed.info} />
        )}
    </>
  );
}

function getTabs(pubkey: PublicKey, account: Account): TabComponent[] {
  const address = pubkey.toBase58();
  const parsedData = account.data.parsed;
  const tabs: Tab[] = [
    {
      slug: "history",
      title: "History",
      path: "",
    },
  ];

  let programTypeKey = "";
  if (parsedData) {
    programTypeKey = `${parsedData.program}:${parsedData.parsed.type}`;
  }

  if (parsedData && parsedData.program in TABS_LOOKUP) {
    tabs.push(...TABS_LOOKUP[parsedData.program]);
  }

  if (parsedData && programTypeKey in TABS_LOOKUP) {
    tabs.push(...TABS_LOOKUP[programTypeKey]);
  }

  // Add the key for address lookup tables
  if (
    account.data.raw &&
    isAddressLookupTableAccount(account.owner, account.data.raw)
  ) {
    tabs.push(...TABS_LOOKUP["address-lookup-table"]);
  }

  // Add the key for Metaplex NFTs
  if (
    parsedData &&
    programTypeKey === "spl-token:mint" &&
    (parsedData as TokenProgramData).nftData
  ) {
    tabs.push(...TABS_LOOKUP[`${programTypeKey}:metaplexNFT`]);
  }

  const isNFToken = account && isNFTokenAccount(account);
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
    (!parsedData ||
      !(
        TOKEN_TABS_HIDDEN.includes(parsedData.program) ||
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
          programId={account.owner}
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
  const anchorProgram = useAnchorProgram(pubkey.toString(), url);

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
  programId: PublicKey;
}) {
  const { url } = useCluster();
  const accountAnchorProgram = useAnchorProgram(programId.toString(), url);

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
