import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { useBlock, useFetchBlock, FetchStatus } from "providers/block";
import { Signature } from "components/common/Signature";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Block } from "components/common/Block";

export function BlockHistoryCard({ block }: { block: string }) {
  const blockData = useBlock(block);
  const fetchBlock = useFetchBlock();
  const refresh = () => fetchBlock(block);

  React.useEffect(() => {
    if (!blockData) refresh();
  }, [blockData, block]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!blockData) {
    return null;
  }

  if (blockData?.data === undefined) {
    if (blockData.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading block" />;
    }

    return <ErrorCard retry={refresh} text="Failed to fetch block" />;
  }

  if (blockData.status === FetchStatus.FetchFailed) {
    return <ErrorCard text={`Block "${block}" is not valid`} />;
  }

  return (
    <>
      {blockData.data?.tags ? (
        <div className="card">
          <div className="card-header">
            <h3 className="card-header-title mb-0 d-flex align-items-center">
              Overview
            </h3>
          </div>

          <TableCardBody>
            {blockData.data?.tags
              .filter(
                (tag: any) =>
                  tag.name === "slot" ||
                  tag.name === "parentSlot" ||
                  tag.name === "blockhash" ||
                  tag.name === "previousBlockhash"
              )
              .map((tag: any) => {
                return (
                  <tr key={tag.name}>
                    <td>
                      {tag.name === "slot" ? "Slot" : ""}
                      {tag.name === "parentSlot" ? "Parent Slot" : ""}
                      {tag.name === "blockhash" ? "Blockhash" : ""}
                      {tag.name === "previousBlockhash"
                        ? "Previous Blockhash"
                        : ""}
                    </td>
                    <td className="text-lg-right">
                      {tag.name === "slot" ? (
                        <Block block={tag.value} alignRight={true} />
                      ) : (
                        ""
                      )}
                      {tag.name === "parentSlot" ? (
                        <Block
                          block={tag.value}
                          alignRight={true}
                          link={true}
                        />
                      ) : (
                        ""
                      )}
                      {tag.name === "blockhash" ? (
                        <Block block={tag.value} alignRight={true} />
                      ) : (
                        ""
                      )}

                      {tag.name === "previousBlockhash" ? (
                        <Block block={tag.value} alignRight={true} />
                      ) : (
                        ""
                      )}
                    </td>
                  </tr>
                );
              })}
          </TableCardBody>
        </div>
      ) : (
        ""
      )}

      {blockData.data?.blockData ? (
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
                {blockData.data?.blockData.transactions.map((tx: any) => {
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
                })}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        ""
      )}
    </>
  );
}
