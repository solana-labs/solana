import React from "react";
import { ConfirmedBlock, PublicKey } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { TableCardBody } from "components/common/TableCardBody";

export function BlockProgramsCard({ block }: { block: ConfirmedBlock }) {
  const totalTransactions = block.transactions.length;
  const txFrequency = new Map<string, number>();
  const ixFrequency = new Map<string, number>();

  let totalInstructions = 0;
  block.transactions.forEach((tx) => {
    totalInstructions += tx.transaction.instructions.length;
    const programUsed = new Set<string>();
    const trackProgramId = (programId: PublicKey) => {
      const programAddress = programId.toBase58();
      programUsed.add(programAddress);
      const frequency = ixFrequency.get(programAddress);
      ixFrequency.set(programAddress, frequency ? frequency + 1 : 1);
    };

    tx.transaction.instructions.forEach((ix, index) => {
      trackProgramId(ix.programId);
      tx.meta?.innerInstructions?.forEach((inner) => {
        if (inner.index !== index) return;
        totalInstructions += inner.instructions.length;
        inner.instructions.forEach((innerIx) => {
          if (innerIx.programIdIndex >= ix.keys.length) return;
          trackProgramId(ix.keys[innerIx.programIdIndex].pubkey);
        });
      });
    });

    programUsed.forEach((programId) => {
      const frequency = txFrequency.get(programId);
      txFrequency.set(programId, frequency ? frequency + 1 : 1);
    });
  });

  const programEntries = [];
  for (let entry of txFrequency) {
    programEntries.push(entry);
  }

  programEntries.sort((a, b) => {
    if (a[1] < b[1]) return 1;
    if (a[1] > b[1]) return -1;
    return 0;
  });

  return (
    <>
      <div className="card">
        <div className="card-header align-items-center">
          <h3 className="card-header-title">Block Program Stats</h3>
        </div>
        <TableCardBody>
          <tr>
            <td className="w-100">Unique Programs Count</td>
            <td className="text-lg-right text-monospace">
              {programEntries.length}
            </td>
          </tr>
          <tr>
            <td className="w-100">Total Instructions</td>
            <td className="text-lg-right text-monospace">
              {totalInstructions}
            </td>
          </tr>
        </TableCardBody>
      </div>
      <div className="card">
        <div className="card-header align-items-center">
          <h3 className="card-header-title">Block Programs</h3>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">Program</th>
                <th className="text-muted">Transaction Count</th>
                <th className="text-muted">% of Total</th>
                <th className="text-muted">Instruction Count</th>
                <th className="text-muted">% of Total</th>
              </tr>
            </thead>
            <tbody>
              {programEntries.map(([programId, txFreq]) => {
                const ixFreq = ixFrequency.get(programId) as number;
                return (
                  <tr key={programId}>
                    <td>
                      <Address pubkey={new PublicKey(programId)} link />
                    </td>
                    <td>{txFreq}</td>
                    <td>{((100 * txFreq) / totalTransactions).toFixed(2)}%</td>
                    <td>{ixFreq}</td>
                    <td>{((100 * ixFreq) / totalInstructions).toFixed(2)}%</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
