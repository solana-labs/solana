import React from "react";
import { PublicKey, VersionedMessage } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { useAddressLookupTable } from "providers/accounts";
import { FetchStatus } from "providers/cache";

export function AddressTableLookupsCard({
  message,
}: {
  message: VersionedMessage;
}) {
  const [expanded, setExpanded] = React.useState(true);

  const lookupRows = React.useMemo(() => {
    let key = 0;
    return message.addressTableLookups.flatMap((lookup) => {
      const indexes = [
        ...lookup.writableIndexes.map((index) => ({ index, readOnly: false })),
        ...lookup.readonlyIndexes.map((index) => ({ index, readOnly: true })),
      ];

      indexes.sort((a, b) => (a.index < b.index ? -1 : 1));

      return indexes.map(({ index, readOnly }) => {
        const props = {
          lookupTableKey: lookup.accountKey,
          lookupTableIndex: index,
          readOnly,
        };
        return <LookupRow key={key++} {...props} />;
      });
    });
  }, [message]);

  if (message.version === "legacy") return null;

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Address Table Lookup(s)</h3>
        <button
          className={`btn btn-sm d-flex ${
            expanded ? "btn-black active" : "btn-white"
          }`}
          onClick={() => setExpanded((e) => !e)}
        >
          {expanded ? "Collapse" : "Expand"}
        </button>
      </div>
      {expanded && (
        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">Address Lookup Table Address</th>
                <th className="text-muted">Table Index</th>
                <th className="text-muted">Resolved Address</th>
                <th className="text-muted">Details</th>
              </tr>
            </thead>
            {lookupRows.length > 0 ? (
              <tbody className="list">{lookupRows}</tbody>
            ) : (
              <div className="card-footer">
                <div className="text-muted text-center">No entries found</div>
              </div>
            )}
          </table>
        </div>
      )}
    </div>
  );
}

function LookupRow({
  lookupTableKey,
  lookupTableIndex,
  readOnly,
}: {
  lookupTableKey: PublicKey;
  lookupTableIndex: number;
  readOnly: boolean;
}) {
  const lookupTableInfo = useAddressLookupTable(lookupTableKey.toBase58());

  const loadingComponent = (
    <span className="text-muted">
      <span className="spinner-grow spinner-grow-sm me-2"></span>
      Loading
    </span>
  );

  let resolvedKeyComponent;
  if (!lookupTableInfo) {
    resolvedKeyComponent = loadingComponent;
  } else {
    const [lookupTable, status] = lookupTableInfo;
    if (status === FetchStatus.Fetching) {
      resolvedKeyComponent = loadingComponent;
    } else if (status === FetchStatus.FetchFailed || !lookupTable) {
      resolvedKeyComponent = (
        <span className="text-muted">Failed to fetch Lookup Table</span>
      );
    } else if (typeof lookupTable === "string") {
      resolvedKeyComponent = (
        <span className="text-muted">Invalid Lookup Table</span>
      );
    } else if (lookupTableIndex >= lookupTable.state.addresses.length) {
      resolvedKeyComponent = (
        <span className="text-muted">Invalid Lookup Table Index</span>
      );
    } else {
      const resolvedKey = lookupTable.state.addresses[lookupTableIndex];
      resolvedKeyComponent = <Address pubkey={resolvedKey} link />;
    }
  }

  return (
    <tr>
      <td className="text-lg-end">
        <Address pubkey={lookupTableKey} link />
      </td>
      <td className="text-lg-end">{lookupTableIndex}</td>
      <td className="text-lg-end">{resolvedKeyComponent}</td>
      <td>
        {!readOnly && <span className="badge bg-info-soft me-1">Writable</span>}
      </td>
    </tr>
  );
}
