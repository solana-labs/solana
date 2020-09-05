import React from "react";
import bs58 from "bs58";
import { useHistory, useLocation } from "react-router-dom";
import Select, { InputActionMeta, ActionMeta, ValueType } from "react-select";
import StateManager from "react-select";
import {
  LOADER_IDS,
  PROGRAM_IDS,
  SYSVAR_IDS,
  ProgramName,
  LoaderName,
} from "utils/tx";
import { TokenRegistry } from "tokenRegistry";
import { Cluster, useCluster } from "providers/cluster";

export function SearchBar() {
  const [search, setSearch] = React.useState("");
  const selectRef = React.useRef<StateManager<any> | null>(null);
  const history = useHistory();
  const location = useLocation();
  const { cluster } = useCluster();

  const onChange = ({ pathname }: ValueType<any>, meta: ActionMeta<any>) => {
    if (meta.action === "select-option") {
      history.push({ ...location, pathname });
      setSearch("");
    }
  };

  const onInputChange = (value: string, { action }: InputActionMeta) => {
    if (action === "input-change") setSearch(value);
  };

  const resetValue = "" as any;
  return (
    <div className="container my-4">
      <div className="row align-items-center">
        <div className="col">
          <Select
            ref={(ref) => (selectRef.current = ref)}
            options={buildOptions(search, cluster)}
            noOptionsMessage={() => "No Results"}
            placeholder="Search for accounts, transactions, programs, and tokens"
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
          />
        </div>
      </div>
    </div>
  );
}

const SEARCHABLE_PROGRAMS: ProgramName[] = [
  "Break Solana Program",
  "Config Program",
  "Stake Program",
  "System Program",
  "Vote Program",
  "SPL Token Program",
];

function buildProgramOptions(search: string) {
  const matchedPrograms = Object.entries(PROGRAM_IDS).filter(
    ([address, name]) => {
      return (
        SEARCHABLE_PROGRAMS.includes(name) &&
        (name.toLowerCase().includes(search.toLowerCase()) ||
          address.includes(search))
      );
    }
  );

  if (matchedPrograms.length > 0) {
    return {
      label: "Programs",
      options: matchedPrograms.map(([id, name]) => ({
        label: name,
        value: [name, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

const SEARCHABLE_LOADERS: LoaderName[] = ["BPF Loader", "BPF Loader 2"];

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

function buildTokenOptions(search: string, cluster: Cluster) {
  const matchedTokens = Object.entries(TokenRegistry.all(cluster)).filter(
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
      options: matchedTokens.map(([id, details]) => ({
        label: details.name,
        value: [details.name, details.symbol, id],
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildOptions(search: string, cluster: Cluster) {
  if (search.length === 0) return [];

  const options = [];

  const programOptions = buildProgramOptions(search);
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

  const tokenOptions = buildTokenOptions(search, cluster);
  if (tokenOptions) {
    options.push(tokenOptions);
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
            value: search,
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
            value: search,
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
