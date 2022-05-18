import React from "react";
import { BlockResponse, PublicKey } from "@solana/web3.js";
import { Address } from "components/common/Address";
import { Link } from "react-router-dom";
import { clusterPath } from "utils/url";

type AccountStats = {
  reads: number;
  writes: number;
};

const PAGE_SIZE = 25;

export function BlockAccountsCard({
  block,
  blockSlot,
}: {
  block: BlockResponse;
  blockSlot: number;
}) {
  const [numDisplayed, setNumDisplayed] = React.useState(10);
  const totalTransactions = block.transactions.length;

  const accountStats = React.useMemo(() => {
    const statsMap = new Map<string, AccountStats>();
    block.transactions.forEach((tx) => {
      const message = tx.transaction.message;
      const txSet = new Map<string, boolean>();
      message.instructions.forEach((ix) => {
        ix.accounts.forEach((index) => {
          const address = message.accountKeys[index].toBase58();
          txSet.set(address, message.isAccountWritable(index));
        });
      });

      txSet.forEach((isWritable, address) => {
        const stats = statsMap.get(address) || { reads: 0, writes: 0 };
        if (isWritable) {
          stats.writes++;
        } else {
          stats.reads++;
        }
        statsMap.set(address, stats);
      });
    });

    const accountEntries = [];
    for (let entry of statsMap) {
      accountEntries.push(entry);
    }

    accountEntries.sort((a, b) => {
      const aCount = a[1].reads + a[1].writes;
      const bCount = b[1].reads + b[1].writes;
      if (aCount < bCount) return 1;
      if (aCount > bCount) return -1;
      return 0;
    });

    return accountEntries;
  }, [block]);

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Block Account Usage</h3>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Account</th>
              <th className="text-muted">Read-Write Count</th>
              <th className="text-muted">Read-Only Count</th>
              <th className="text-muted">Total Count</th>
              <th className="text-muted">% of Transactions</th>
            </tr>
          </thead>
          <tbody>
            {accountStats
              .slice(0, numDisplayed)
              .map(([address, { writes, reads }]) => {
                return (
                  <tr key={address}>
                    <td>
                      <Link
                        to={clusterPath(
                          `/block/${blockSlot}`,
                          new URLSearchParams(
                            `accountFilter=${address}&filter=all`
                          )
                        )}
                      >
                        <Address pubkey={new PublicKey(address)} />
                      </Link>
                    </td>
                    <td>{writes}</td>
                    <td>{reads}</td>
                    <td>{writes + reads}</td>
                    <td>
                      {((100 * (writes + reads)) / totalTransactions).toFixed(
                        2
                      )}
                      %
                    </td>
                  </tr>
                );
              })}
          </tbody>
        </table>
      </div>

      {accountStats.length > numDisplayed && (
        <div className="card-footer">
          <button
            className="btn btn-primary w-100"
            onClick={() =>
              setNumDisplayed((displayed) => displayed + PAGE_SIZE)
            }
          >
            Load More
          </button>
        </div>
      )}
    </div>
  );
}
