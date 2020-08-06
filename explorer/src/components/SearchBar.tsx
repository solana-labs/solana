import React from "react";
import bs58 from "bs58";
import { useHistory, useLocation } from "react-router-dom";
import Select, { InputActionMeta, ActionMeta, ValueType } from "react-select";
import StateManager from "react-select";

export function SearchBar() {
  const [search, setSearch] = React.useState("");
  const selectRef = React.useRef<StateManager<any> | null>(null);
  const history = useHistory();
  const location = useLocation();

  const onChange = (
    { value: pathname }: ValueType<any>,
    meta: ActionMeta<any>
  ) => {
    if (meta.action === "select-option") {
      history.push({ ...location, pathname });
      setSearch("");
    }
  };

  const onInputChange = (value: string, { action }: InputActionMeta) => {
    if (action === "input-change") setSearch(value);
  };

  const options = ((searchValue: string) => {
    try {
      const decoded = bs58.decode(searchValue);
      if (decoded.length === 32) {
        return [
          {
            label: "Account",
            options: [
              {
                label: searchValue,
                value: "/address/" + searchValue,
              },
            ],
          },
        ];
      } else if (decoded.length === 64) {
        return [
          {
            label: "Transaction",
            options: [
              {
                label: searchValue,
                value: "/tx/" + searchValue,
              },
            ],
          },
        ];
      }
    } catch (err) {}
    return [];
  })(search);

  const resetValue = "" as any;
  return (
    <div className="container my-4">
      <div className="row align-items-center">
        <div className="col">
          <Select
            ref={(ref) => (selectRef.current = ref)}
            options={options}
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

function DropdownIndicator() {
  return (
    <div className="search-indicator">
      <span className="fe fe-search"></span>
    </div>
  );
}
