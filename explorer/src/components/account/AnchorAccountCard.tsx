import React, { useMemo } from "react";

import { Account } from "providers/accounts";
import { Address } from "components/common/Address";
import { BorshAccountsCoder } from "@project-serum/anchor";
import { capitalizeFirstLetter } from "utils/anchor";
import { ErrorCard } from "components/common/ErrorCard";
import { PublicKey } from "@solana/web3.js";
import BN from "bn.js";

import ReactJson from "react-json-view";
import { useCluster } from "providers/cluster";
import { useAnchorProgram } from "providers/anchor";

export function AnchorAccountCard({ account }: { account: Account }) {
  const { url } = useCluster();
  const program = useAnchorProgram(
    account.details?.owner.toString() ?? "",
    url
  );

  const { foundAccountLayoutName, decodedAnchorAccountData } = useMemo(() => {
    let foundAccountLayoutName: string | undefined;
    let decodedAnchorAccountData: { [key: string]: any } | undefined;
    if (program && account.details && account.details.rawData) {
      const accountBuffer = account.details.rawData;
      const discriminator = accountBuffer.slice(0, 8);

      // Iterate all the structs, see if any of the name-hashes match
      Object.keys(program.account).forEach((accountType) => {
        const layoutName = capitalizeFirstLetter(accountType);
        const discriminatorToCheck =
          BorshAccountsCoder.accountDiscriminator(layoutName);

        if (discriminatorToCheck.equals(discriminator)) {
          foundAccountLayoutName = layoutName;
          const accountDecoder = program.account[accountType];
          decodedAnchorAccountData = accountDecoder.coder.accounts.decode(
            layoutName,
            accountBuffer
          );
        }
      });
    }
    return { foundAccountLayoutName, decodedAnchorAccountData };
  }, [program, account.details]);

  if (!foundAccountLayoutName || !decodedAnchorAccountData) {
    return (
      <ErrorCard text="Failed to decode account data according to its public anchor interface" />
    );
  }

  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">{foundAccountLayoutName}</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Key</th>
                <th className="text-muted">Value</th>
              </tr>
            </thead>
            <tbody className="list">
              {decodedAnchorAccountData &&
                Object.keys(decodedAnchorAccountData).map((key) => (
                  <AccountRow
                    key={key}
                    valueName={key}
                    value={decodedAnchorAccountData[key]}
                  />
                ))}
            </tbody>
          </table>
        </div>
        <div className="card-footer">
          <div className="text-muted text-center">
            {decodedAnchorAccountData &&
            Object.keys(decodedAnchorAccountData).length > 0
              ? `Decoded ${Object.keys(decodedAnchorAccountData).length} Items`
              : "No decoded data"}
          </div>
        </div>
      </div>
    </>
  );
}

function AccountRow({ valueName, value }: { valueName: string; value: any }) {
  let displayValue: JSX.Element;
  if (value instanceof PublicKey) {
    displayValue = <Address pubkey={value} link />;
  } else if (value instanceof BN) {
    displayValue = <>{value.toString()}</>;
  } else if (!(value instanceof Object)) {
    displayValue = <>{String(value)}</>;
  } else if (value) {
    const displayObject = stringifyPubkeyAndBigNums(value);
    displayValue = (
      <ReactJson
        src={JSON.parse(JSON.stringify(displayObject))}
        collapsed={1}
        theme="solarized"
      />
    );
  } else {
    displayValue = <>null</>;
  }
  return (
    <tr>
      <td className="w-1 text-monospace">{camelToUnderscore(valueName)}</td>
      <td className="text-monospace">{displayValue}</td>
    </tr>
  );
}

function camelToUnderscore(key: string) {
  var result = key.replace(/([A-Z])/g, " $1");
  return result.split(" ").join("_").toLowerCase();
}

function stringifyPubkeyAndBigNums(object: Object): Object {
  if (!Array.isArray(object)) {
    if (object instanceof PublicKey) {
      return object.toString();
    } else if (object instanceof BN) {
      return object.toString();
    } else if (!(object instanceof Object)) {
      return object;
    } else {
      const parsedObject: { [key: string]: Object } = {};
      Object.keys(object).map((key) => {
        let value = (object as { [key: string]: any })[key];
        if (value instanceof Object) {
          value = stringifyPubkeyAndBigNums(value);
        }
        parsedObject[key] = value;
        return null;
      });
      return parsedObject;
    }
  }
  return object.map((innerObject) =>
    innerObject instanceof Object
      ? stringifyPubkeyAndBigNums(innerObject)
      : innerObject
  );
}
