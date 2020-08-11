import React from "react";
import bs58 from "bs58";
import { useHistory, useLocation } from "react-router-dom";
import Select, { InputActionMeta, ActionMeta, ValueType } from "react-select";
import StateManager from "react-select";
import { PROGRAM_IDS, SYSVAR_IDS } from "utils/tx";

export function SearchBar() {
  const [search, setSearch] = React.useState("");
  const selectRef = React.useRef<StateManager<any> | null>(null);
  const history = useHistory();
  const location = useLocation();

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
            options={buildOptions(search)}
            noOptionsMessage={() => "No Results"}
            placeholder="Search by address or signature"
            value={resetValue}
            inputValue={search}
            blurInputOnSelect
            onMenuClose={() => selectRef.current?.blur()}
            onChange={onChange}
            onInputChange={onInputChange}
            components={{ DropdownIndicator }}
            classNamePrefix="search-bar"
          />
        </div>
      </div>
    </div>
  );
}

const SEARCHABLE_PROGRAMS = ["Config", "Stake", "System", "Vote", "Token"];

function buildProgramOptions(search: string) {
  const matchedPrograms = Object.entries(PROGRAM_IDS).filter(([, name]) => {
    return (
      SEARCHABLE_PROGRAMS.includes(name) &&
      name.toLowerCase().includes(search.toLowerCase())
    );
  });

  if (matchedPrograms.length > 0) {
    return {
      label: "Programs",
      options: matchedPrograms.map(([id, name]) => ({
        label: name,
        value: name,
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildSysvarOptions(search: string) {
  const matchedSysvars = Object.entries(SYSVAR_IDS).filter(([, name]) => {
    return name.toLowerCase().includes(search.toLowerCase());
  });

  if (matchedSysvars.length > 0) {
    return {
      label: "Sysvars",
      options: matchedSysvars.map(([id, name]) => ({
        label: name,
        value: name,
        pathname: "/address/" + id,
      })),
    };
  }
}

function buildOptions(search: string) {
  if (search.length === 0) return [];

  const options = [];

  const programOptions = buildProgramOptions(search);
  if (programOptions) {
    options.push(programOptions);
  }

  const sysvarOptions = buildSysvarOptions(search);
  if (sysvarOptions) {
    options.push(sysvarOptions);
  }

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
