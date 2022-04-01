import React from "react";

import { Account } from "providers/accounts";
import { Address } from "components/common/Address";
import { Program, BorshAccountsCoder } from "@project-serum/anchor";
import { capitalizeFirstLetter } from "utils/anchor";
import { ErrorCard } from "components/common/ErrorCard";

export function AnchorAccountCard({
  account,
  program,
}: {
  account: Account;
  program: Program;
}) {
  if (!account.details || !account.details.rawData) {
    return (
      <ErrorCard text={"This account is parsed as an SPL-native account"} />
    );
  }
  const accountBuffer = account.details.rawData;

  const discriminator = accountBuffer.slice(0, 8) ?? undefined;
  if (!discriminator) {
    return <ErrorCard text={"Failed to find anchor account discriminator"} />;
  }

  let foundAccountLayoutName: string | undefined;
  let decodedAnchorAccountData: Object | undefined;
  if (program) {
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

  if (!foundAccountLayoutName || !decodedAnchorAccountData) {
    return (
      <ErrorCard
        text={
          "Failed to find matching anchor account type for account discriminator"
        }
      />
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
                Object.keys(decodedAnchorAccountData).map((key) => {
                  // @ts-ignore
                  return renderAccountRow(key, decodedAnchorAccountData[key]);
                })}
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

const renderAccountRow = (key: string, value: any) => {
  let displayValue = value.toString();
  if (value.constructor.name === "PublicKey") {
    displayValue = <Address pubkey={value} link />;
  } else if (displayValue === {}.toString()) {
    if (Object.keys(value).length === 1) {
      displayValue = Object.keys(value)[0];
    } else {
      displayValue = JSON.stringify(value);
    }
  }
  return (
    <tr key={key}>
      <td className="w-1 text-monospace">{camelToUnderscore(key)}</td>
      <td className="text-monospace">{displayValue}</td>
    </tr>
  );
};

function camelToUnderscore(key: string) {
  var result = key.replace(/([A-Z])/g, " $1");
  return result.split(" ").join("_").toLowerCase();
}
