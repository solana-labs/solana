import React from "react";
import bs58 from "bs58";
import { useHistory, useLocation } from "react-router-dom";
import Select, { InputActionMeta, ActionMeta, ValueType } from "react-select";
import StateManager from "react-select";
import {
  LOADER_IDS,
  PROGRAM_INFO_BY_ID,
  SPECIAL_IDS,
  SYSVAR_IDS,
  LoaderName,
} from "utils/tx";
import { Cluster, useCluster } from "providers/cluster";
import { useTokenRegistry } from "providers/mints/token-registry";
import { TokenInfoMap } from "@solana/spl-token-registry";
import { Connection } from "@solana/web3.js";
import { getDomainInfo, hasDomainSyntax } from "utils/name-service";

interface SearchOptions {
  label: string;
  options: {
    label: string;
    value: string[];
    pathname: string;
  }[];
}

export function SearchBar() {
  const [search, setSearch] = React.useState("");
  const searchRef = React.useRef("");
  const [searchOptions, setSearchOptions] = React.useState<SearchOptions[]>([]);
  const [loadingSearch, setLoadingSearch] = React.useState<boolean>(false);
  const [loadingSearchMessage, setLoadingSearchMessage] =
    React.useState<string>("loading...");
  const selectRef = React.useRef<StateManager<any> | null>(null);
  const history = useHistory();
  const location = useLocation();
  const { tokenRegistry } = useTokenRegistry();
  const { url, cluster, clusterInfo } = useCluster();

  const onChange = (
    { pathname }: ValueType<any, false>,
    meta: ActionMeta<any>
  ) => {
    if (meta.action === "select-option") {
      history.push({ ...location, pathname });
      setSearch("");
    }
  };

  const onInputChange = (value: string, { action }: InputActionMeta) => {
    if (action === "input-change") {
      setSearch(value);
    }
  };

  React.useEffect(() => {
    searchRef.current = search;
    setLoadingSearchMessage("Loading...");
    setLoadingSearch(true);

    // builds and sets local search output
    const options = buildOptions(
      search,
      cluster,
      tokenRegistry,
      clusterInfo?.epochInfo.epoch
    );

    setSearchOptions(options);

    // checking for non local search output
    if (hasDomainSyntax(search)) {
      // if search input is a potential domain we continue the loading state
      domainSearch(options);
    } else {
      // if search input is not a potential domain we can conclude the search has finished
      setLoadingSearch(false);
    }

    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  // appends domain lookup results to the local search state
  const domainSearch = async (options: SearchOptions[]) => {
    setLoadingSearchMessage("Looking up domain...");
    const connection = new Connection(url);
    const searchTerm = search;
    const updatedOptions = await buildDomainOptions(
      connection,
      search,
      options
    );
    if (searchRef.current === searchTerm) {
      setSearchOptions(updatedOptions);
      // after attempting to fetch the domain name we can conclude the loading state
      setLoadingSearch(false);
      setLoadingSearchMessage("Loading...");
    }
  };

  const resetValue = "" as any;
  return (
    <div className="container my-4">
      <div className="row align-items-center">
        <div className="col">
          <Select
            autoFocus
            ref={(ref) => (selectRef.current = ref)}
            options={searchOptions}
            noOptionsMessage={() => "No Results"}
            loadingMessage={() => loadingSearchMessage}
            placeholder="Search for blocks, accounts, transactions, programs, and tokens"
            value={resetValue}
            inputValue={search}
            blurInputOnSelect
            onMenuClose={() => selectRef.current?.blur()}
            onChange={onChange}
            styles={{
              /* work around for https://github.com/JedWatson/react-select/issues/3857 */
              placeholder: (style) => ({ ...style, pointerEvents: "none" }),
              input: (style) => ({ ...style, width: "100%" }),
            }}
            onInputChange={onInputChange}
            components={{ DropdownIndicator }}
            classNamePrefix="search-bar"
            isLoading={loadingSearch}
          />
        </div>
      </div>
    </div>
  );
}

function buildProgramOptions(search: string, cluster: Cluster) {
  const matchedPrograms = Object.entries(PROGRAM_INFO_BY_ID).filter(
    ([address, { name, deployments }]) => {
      if (!deployments.includes(cluster)) return false;
      return (
        name.toLowerCase().includes(search.toLowerCase()) ||
        address.includes(search)
      );
    }
  );

  if (matchedPrograms.length > 0) {
    return {
      label: "Programs",
      options: matchedPrograms.map(([address, { name }]) => ({
        label: name,
        value: [name, address],
        pathname: "/address/" + address,
      })),
    };
  }
}

const SEARCHABLE_LOADERS: LoaderName[] = [
  "BPF Loader",
  "BPF Loader 2",
  "BPF Upgradeable Loader",
];

function buildLoaderOptions(search: string) {
  const matchedLoaders = Object.entries(LOADER_IDS).filter(
    ([address, name]) => {
      return (
        SEARCHABLE_LOADERS.includes(name) &&
        (name.toLowerCase().includes(search.toLowerCase()) ||
          address.includes(search))
      );
    }
  );

  if (matchedLoaders.length > 0) {
    return {
      label: "Program Loaders",
      options: matchedLoaders.map(([id, name]) => ({
        label: name,
        value: [name, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildSysvarOptions(search: string) {
  const matchedSysvars = Object.entries(SYSVAR_IDS).filter(
    ([address, name]) => {
      return (
        name.toLowerCase().includes(search.toLowerCase()) ||
        address.includes(search)
      );
    }
  );

  if (matchedSysvars.length > 0) {
    return {
      label: "Sysvars",
      options: matchedSysvars.map(([id, name]) => ({
        label: name,
        value: [name, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildSpecialOptions(search: string) {
  const matchedSpecialIds = Object.entries(SPECIAL_IDS).filter(
    ([address, name]) => {
      return (
        name.toLowerCase().includes(search.toLowerCase()) ||
        address.includes(search)
      );
    }
  );

  if (matchedSpecialIds.length > 0) {
    return {
      label: "Accounts",
      options: matchedSpecialIds.map(([id, name]) => ({
        label: name,
        value: [name, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildTokenOptions(
  search: string,
  cluster: Cluster,
  tokenRegistry: TokenInfoMap
) {
  const matchedTokens = Array.from(tokenRegistry.entries()).filter(
    ([address, details]) => {
      const searchLower = search.toLowerCase();
      return (
        details.name.toLowerCase().includes(searchLower) ||
        details.symbol.toLowerCase().includes(searchLower) ||
        address.includes(search)
      );
    }
  );

  if (matchedTokens.length > 0) {
    return {
      label: "Tokens",
      options: matchedTokens.slice(0, 10).map(([id, details]) => ({
        label: details.name,
        value: [details.name, details.symbol, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

async function buildDomainOptions(
  connection: Connection,
  search: string,
  options: SearchOptions[]
) {
  const domainInfo = await getDomainInfo(search, connection);
  const updatedOptions: SearchOptions[] = [...options];
  if (domainInfo && domainInfo.owner && domainInfo.address) {
    updatedOptions.push({
      label: "Domain Owner",
      options: [
        {
          label: domainInfo.owner,
          value: [search],
          pathname: "/address/" + domainInfo.owner,
        },
      ],
    });
    updatedOptions.push({
      label: "Name Service Account",
      options: [
        {
          label: search,
          value: [search],
          pathname: "/address/" + domainInfo.address,
        },
      ],
    });
  }
  return updatedOptions;
}

// builds local search options
function buildOptions(
  rawSearch: string,
  cluster: Cluster,
  tokenRegistry: TokenInfoMap,
  currentEpoch?: number
) {
  const search = rawSearch.trim();
  if (search.length === 0) return [];

  const options = [];

  const programOptions = buildProgramOptions(search, cluster);
  if (programOptions) {
    options.push(programOptions);
  }

  const loaderOptions = buildLoaderOptions(search);
  if (loaderOptions) {
    options.push(loaderOptions);
  }

  const sysvarOptions = buildSysvarOptions(search);
  if (sysvarOptions) {
    options.push(sysvarOptions);
  }

  const specialOptions = buildSpecialOptions(search);
  if (specialOptions) {
    options.push(specialOptions);
  }

  const tokenOptions = buildTokenOptions(search, cluster, tokenRegistry);
  if (tokenOptions) {
    options.push(tokenOptions);
  }

  if (!isNaN(Number(search))) {
    options.push({
      label: "Block",
      options: [
        {
          label: `Slot #${search}`,
          value: [search],
          pathname: `/block/${search}`,
        },
      ],
    });

    if (currentEpoch !== undefined && Number(search) <= currentEpoch + 1) {
      options.push({
        label: "Epoch",
        options: [
          {
            label: `Epoch #${search}`,
            value: [search],
            pathname: `/epoch/${search}`,
          },
        ],
      });
    }
  }

  // Prefer nice suggestions over raw suggestions
  if (options.length > 0) return options;

  try {
    const decoded = bs58.decode(search);
    if (decoded.length === 32) {
      options.push({
        label: "Account",
        options: [
          {
            label: search,
            value: [search],
            pathname: "/address/" + search,
          },
        ],
      });
    } else if (decoded.length === 64) {
      options.push({
        label: "Transaction",
        options: [
          {
            label: search,
            value: [search],
            pathname: "/tx/" + search,
          },
        ],
      });
    }
  } catch (err) {}

  return options;
}

function DropdownIndicator() {
  return (
    <div className="search-indicator">
      <span className="fe fe-search"></span>
    </div>
  );
}
