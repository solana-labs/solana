import React from "react";

import { Account } from "providers/accounts";
import { useCluster } from "providers/cluster";
import { Address } from "components/common/Address";
import { Program, Provider, BorshAccountsCoder } from "@project-serum/anchor";

import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";

import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import { useAnchorProgram } from "providers/anchor";

export function AnchorAccountCard({ account, program }: { account: Account, program: Program }) {
  const { url } = useCluster();
  const [decodedAnchorAccountData, setDecodedAnchorAccountData] =
    React.useState<any>();
  const [decodedAnchorAccountName, setDecodedAnchorAccountName] =
    React.useState<string>();

  React.useEffect(() => {
    setDecodedAnchorAccountData(undefined);
    (async () => {
      const connection = new Connection(url);
      const provider = new Provider(
        connection,
        new NodeWallet(Keypair.generate()),
        {
          skipPreflight: false,
          commitment: "confirmed",
          preflightCommitment: "confirmed",
        }
      );

      if (!account.details) {
        return;
      }

      // TODO: this should be cached
      const accountInfo = await connection.getAccountInfo(account.pubkey);
      const discriminator = accountInfo?.data.slice(0, 8);
      if (!discriminator) {
        return;
      }

      if (program && accountInfo && accountInfo.data) {
        // Iterate all the structs, see if any of the name-hashes match
        Object.keys(program.account).forEach((accountType) => {
          const layoutName = capitalizeFirstLetter(accountType);
          const discriminatorToCheck =
            BorshAccountsCoder.accountDiscriminator(layoutName);

          if (equal(discriminatorToCheck, discriminator)) {
            const accountDecoder = program.account[accountType];
            const decodedAnchorObject =
              accountDecoder.coder.accounts.decode(
                layoutName,
                accountInfo.data
              );

            setDecodedAnchorAccountName(layoutName);
            setDecodedAnchorAccountData(decodedAnchorObject);
          }
        });
      }
    })();
  }, [account, url]);

  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">{decodedAnchorAccountName}</h3>
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

export const hasAnchorIDL = async (
  address: PublicKey,
  url: string
): Promise<boolean> => {
  const connection = new Connection(url);
  const provider = new Provider(
    connection,
    new NodeWallet(Keypair.generate()),
    {
      skipPreflight: false,
      commitment: "confirmed",
      preflightCommitment: "confirmed",
    }
  );

  const program = await Program.at(address, provider).catch(() => {});
  return !!program;
};

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

function capitalizeFirstLetter(input: string) {
  return input.charAt(0).toUpperCase() + input.slice(1);
}

function equal(buf1: Buffer, buf2: Buffer) {
  if (buf1.byteLength !== buf2.byteLength) return false;
  var dv1 = new Int8Array(buf1);
  var dv2 = new Int8Array(buf2);
  for (var i = 0; i !== buf1.byteLength; i++) {
    if (dv1[i] !== dv2[i]) return false;
  }
  return true;
}

function camelToUnderscore(key: string) {
  var result = key.replace(/([A-Z])/g, " $1");
  return result.split(" ").join("_").toLowerCase();
}
