import React from "react";
import { PublicKey, VersionedMessage } from "@solana/web3.js";
import { TableCardBody } from "components/common/TableCardBody";
import {
  AddressFromLookupTableWithContext,
  AddressWithContext,
} from "./AddressWithContext";
import { ErrorCard } from "components/common/ErrorCard";

export function AccountsCard({ message }: { message: VersionedMessage }) {
  const [expanded, setExpanded] = React.useState(true);

  const { validMessage, error } = React.useMemo(() => {
    const {
      numRequiredSignatures,
      numReadonlySignedAccounts,
      numReadonlyUnsignedAccounts,
    } = message.header;

    if (numReadonlySignedAccounts >= numRequiredSignatures) {
      return { validMessage: undefined, error: "Invalid header" };
    } else if (
      numReadonlyUnsignedAccounts >= message.staticAccountKeys.length
    ) {
      return { validMessage: undefined, error: "Invalid header" };
    } else if (message.staticAccountKeys.length === 0) {
      return { validMessage: undefined, error: "Message has no accounts" };
    }

    return {
      validMessage: message,
      error: undefined,
    };
  }, [message]);

  const { accountRows, numAccounts } = React.useMemo(() => {
    const message = validMessage;
    if (!message) return { accountRows: undefined, numAccounts: 0 };
    const staticAccountRows = message.staticAccountKeys.map(
      (publicKey, accountIndex) => {
        const {
          numRequiredSignatures,
          numReadonlySignedAccounts,
          numReadonlyUnsignedAccounts,
        } = message.header;

        let readOnly = false;
        let signer = false;
        if (accountIndex < numRequiredSignatures) {
          signer = true;
          if (
            accountIndex >=
            numRequiredSignatures - numReadonlySignedAccounts
          ) {
            readOnly = true;
          }
        } else if (
          accountIndex >=
          message.staticAccountKeys.length - numReadonlyUnsignedAccounts
        ) {
          readOnly = true;
        }

        const props = {
          accountIndex,
          publicKey,
          signer,
          readOnly,
        };

        return <AccountRow key={accountIndex} {...props} />;
      }
    );

    let accountIndex = message.staticAccountKeys.length;
    const writableLookupTableRows = message.addressTableLookups.flatMap(
      (lookup) => {
        return lookup.writableIndexes.map((lookupTableIndex) => {
          const props = {
            accountIndex,
            lookupTableKey: lookup.accountKey,
            lookupTableIndex,
            readOnly: false,
          };

          accountIndex += 1;
          return <AccountFromLookupTableRow key={accountIndex} {...props} />;
        });
      }
    );

    const readonlyLookupTableRows = message.addressTableLookups.flatMap(
      (lookup) => {
        return lookup.readonlyIndexes.map((lookupTableIndex) => {
          const props = {
            accountIndex,
            lookupTableKey: lookup.accountKey,
            lookupTableIndex,
            readOnly: true,
          };

          accountIndex += 1;
          return <AccountFromLookupTableRow key={accountIndex} {...props} />;
        });
      }
    );

    return {
      accountRows: [
        ...staticAccountRows,
        ...writableLookupTableRows,
        ...readonlyLookupTableRows,
      ],
      numAccounts: accountIndex,
    };
  }, [validMessage]);

  if (error) {
    return <ErrorCard text={`Unable to display accounts. ${error}`} />;
  }

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">{`Account List (${numAccounts})`}</h3>
        <button
          className={`btn btn-sm d-flex ${
            expanded ? "btn-black active" : "btn-white"
          }`}
          onClick={() => setExpanded((e) => !e)}
        >
          {expanded ? "Collapse" : "Expand"}
        </button>
      </div>
      {expanded && <TableCardBody>{accountRows}</TableCardBody>}
    </div>
  );
}

function AccountFromLookupTableRow({
  accountIndex,
  lookupTableKey,
  lookupTableIndex,
  readOnly,
}: {
  accountIndex: number;
  lookupTableKey: PublicKey;
  lookupTableIndex: number;
  readOnly: boolean;
}) {
  return (
    <tr>
      <td>
        <div className="d-flex align-items-start flex-column">
          Account #{accountIndex + 1}
          <span className="mt-1">
            {!readOnly && (
              <span className="badge bg-danger-soft me-1">Writable</span>
            )}
            <span className="badge bg-gray-soft">Address Table Lookup</span>
          </span>
        </div>
      </td>
      <td className="text-lg-end">
        <AddressFromLookupTableWithContext
          lookupTableKey={lookupTableKey}
          lookupTableIndex={lookupTableIndex}
        />
      </td>
    </tr>
  );
}

function AccountRow({
  accountIndex,
  publicKey,
  signer,
  readOnly,
}: {
  accountIndex: number;
  publicKey: PublicKey;
  signer: boolean;
  readOnly: boolean;
}) {
  return (
    <tr>
      <td>
        <div className="d-flex align-items-start flex-column">
          Account #{accountIndex + 1}
          <span className="mt-1">
            {signer && <span className="badge bg-info-soft me-1">Signer</span>}
            {!readOnly && (
              <span className="badge bg-danger-soft">Writable</span>
            )}
          </span>
        </div>
      </td>
      <td className="text-lg-end">
        <AddressWithContext pubkey={publicKey} />
      </td>
    </tr>
  );
}
