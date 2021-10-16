import React from "react";
import { TransactionInstruction } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { HexData } from "components/common/HexData";
import { AccountName, capitalizeFirstLetter, getAnchorAccountsFromInstruction } from "utils/anchor";
import { useCluster } from "providers/cluster";
import { BorshInstructionCoder } from "@project-serum/anchor";
import { useAnchorProgram } from "providers/anchor";

export function RawDetails({ ix }: { ix: TransactionInstruction }) {
  const { cluster, url } = useCluster();
  const program = useAnchorProgram(ix.programId.toString(), url);

  let ixAccounts: {
    name: string;
    isMut: boolean;
    isSigner: boolean;
    pda?: Object;
  }[] | null = null;
  if (program) {
    ixAccounts = getAnchorAccountsFromInstruction(ix, program);
  }

  return (
    <>
      {ix.keys.map(({ pubkey, isSigner, isWritable }, keyIndex) => {
        return (
          <tr key={keyIndex}>
            <td>
              <div className="me-2 d-md-inline">
                {/* <AccountName accountPubkey={pubkey} programId={ix.programId} cluster={cluster} defaultAccountName={`Account #${keyIndex + 1}`} /> */}
                {ixAccounts && keyIndex < ixAccounts.length ? `${capitalizeFirstLetter(ixAccounts[keyIndex].name)}` : `Account #${keyIndex+1}`}
              </div>
              {isWritable && (
                <span className="badge bg-info-soft me-1">Writable</span>
              )}
              {isSigner && (
                <span className="badge bg-info-soft me-1">Signer</span>
              )}
            </td>
            <td className="text-lg-end">
              <Address pubkey={pubkey} alignRight link />
            </td>
          </tr>
        )
      })}

      <tr>
        <td>
          Instruction Data <span className="text-muted">(Hex)</span>
        </td>
        <td className="text-lg-end">
          <HexData raw={ix.data} />
        </td>
      </tr>
    </>
  );
}
