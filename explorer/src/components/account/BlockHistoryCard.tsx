import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock } from "providers/accounts/block";
import { Signature } from "components/common/Signature";

export function BlockHistoryCard({
  slot,
  blockhash,
}: {
  slot: string;
  blockhash: string;
}) {
  const block = useBlock(slot ? slot : blockhash);
  const fetchBlock = useFetchBlock();
  const refresh = () =>
    fetchBlock(slot ? slot : blockhash, slot ? "slot" : "blockhash");

  React.useEffect(() => {
    if (!block) refresh();
  }, [slot, blockhash, block]); // eslint-disable-line react-hooks/exhaustive-deps

  function invalidOverview() {
    return (
      <tr>
        <td>ERROR</td>
        <td>Could not retrieve block details</td>
      </tr>
    );
  }

  function invalidTransactions() {
    return (
      <tr>
        <td>ERROR</td>
        <td>Could not retrieve transactions</td>
      </tr>
    );
  }

  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Overview
          </h3>
        </div>

        <TableCardBody>
          {block
            ? block.data?.Tags
              ? block.data?.Tags.filter(
                  (tag: any) =>
                    tag.name === "slot" ||
                    tag.name === "parentSlot" ||
                    tag.name === "blockhash"
                ).map((tag: any) => {
                  return (
                    <tr>
                      <td>{tag.name}</td>
                      <td className="text-lg-right">{tag.value}</td>
                    </tr>
                  );
                })
              : invalidOverview()
            : invalidOverview()}
        </TableCardBody>
      </div>

      <div className="card">
        <div className="card-header align-items-center">
          <h3 className="card-header-title">Block Transactions</h3>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">Result</th>
                <th className="text-muted">Transaction Signature</th>
              </tr>
            </thead>
            <tbody className="list">
              {block
                ? block.data?.BlockData
                  ? block.data?.BlockData.transactions.map((tx: any) => {
                      return (
                        <tr key={tx.transaction.signatures[0]}>
                          <td>
                            <span className={`badge badge-soft-success`}>
                              success
                            </span>
                          </td>

                          <td>
                            <Signature
                              signature={tx.transaction.signatures[0]}
                              link
                            />
                          </td>
                        </tr>
                      );
                    })
                  : invalidTransactions()
                : invalidTransactions()}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
