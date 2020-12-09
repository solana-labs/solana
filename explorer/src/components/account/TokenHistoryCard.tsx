import React from "react";
import {
  PublicKey,
  ConfirmedSignatureInfo,
  ParsedInstruction,
  PartiallyDecodedInstruction,
} from "@solana/web3.js";
import { CacheEntry, FetchStatus } from "providers/cache";
import {
  useAccountHistories,
  useFetchAccountHistory,
} from "providers/accounts/history";
import {
  useAccountOwnedTokens,
  TokenInfoWithPubkey,
  TOKEN_PROGRAM_ID,
} from "providers/accounts/tokens";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Signature } from "components/common/Signature";
import { Address } from "components/common/Address";
import { Slot } from "components/common/Slot";
import {
  Details,
  useFetchTransactionDetails,
  useTransactionDetailsCache,
} from "providers/transactions/details";
import { coerce } from "superstruct";
import { ParsedInfo } from "validators";
import {
  TokenInstructionType,
  IX_TITLES,
} from "components/instruction/token/types";
import { reportError } from "utils/sentry";
import { intoTransactionInstruction } from "utils/tx";
import {
  isTokenSwapInstruction,
  parseTokenSwapInstructionTitle,
} from "components/instruction/token-swap/types";
import {
  isSerumInstruction,
  parseSerumInstructionTitle,
} from "components/instruction/serum/types";
import { INNER_INSTRUCTIONS_START_SLOT } from "pages/TransactionDetailsPage";
import { useCluster, Cluster } from "providers/cluster";

type InstructionType = {
  name: string;
  innerInstructions: (ParsedInstruction | PartiallyDecodedInstruction)[];
};

export function TokenHistoryCard({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const ownedTokens = useAccountOwnedTokens(address);

  if (ownedTokens === undefined) {
    return null;
  }

  const tokens = ownedTokens.data?.tokens;
  if (tokens === undefined || tokens.length === 0) return null;

  if (tokens.length > 25) {
    return (
      <ErrorCard text="Token transaction history is not available for accounts with over 25 token accounts" />
    );
  }

  return <TokenHistoryTable tokens={tokens} />;
}

function TokenHistoryTable({ tokens }: { tokens: TokenInfoWithPubkey[] }) {
  const accountHistories = useAccountHistories();
  const fetchAccountHistory = useFetchAccountHistory();
  const transactionDetailsCache = useTransactionDetailsCache();

  const fetchHistories = (refresh?: boolean) => {
    tokens.forEach((token) => {
      fetchAccountHistory(token.pubkey, refresh);
    });
  };

  // Fetch histories on load
  React.useEffect(() => {
    tokens.forEach((token) => {
      const address = token.pubkey.toBase58();
      if (!accountHistories[address]) {
        fetchAccountHistory(token.pubkey, true);
      }
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const allFoundOldest = tokens.every((token) => {
    const history = accountHistories[token.pubkey.toBase58()];
    return history?.data?.foundOldest === true;
  });

  const allFetchedSome = tokens.every((token) => {
    const history = accountHistories[token.pubkey.toBase58()];
    return history?.data !== undefined;
  });

  // Find the oldest slot which we know we have the full history for
  let oldestSlot: number | undefined = allFoundOldest ? 0 : undefined;
  if (!allFoundOldest && allFetchedSome) {
    tokens.forEach((token) => {
      const history = accountHistories[token.pubkey.toBase58()];
      if (history?.data?.foundOldest === false) {
        const earliest =
          history.data.fetched[history.data.fetched.length - 1].slot;
        if (!oldestSlot) oldestSlot = earliest;
        oldestSlot = Math.max(oldestSlot, earliest);
      }
    });
  }

  const fetching = tokens.some((token) => {
    const history = accountHistories[token.pubkey.toBase58()];
    return history?.status === FetchStatus.Fetching;
  });

  const failed = tokens.some((token) => {
    const history = accountHistories[token.pubkey.toBase58()];
    return history?.status === FetchStatus.FetchFailed;
  });

  const sigSet = new Set();
  const mintAndTxs = tokens
    .map((token) => ({
      mint: token.info.mint,
      history: accountHistories[token.pubkey.toBase58()],
    }))
    .filter(({ history }) => {
      return history?.data?.fetched && history.data.fetched.length > 0;
    })
    .flatMap(({ mint, history }) =>
      (history?.data?.fetched as ConfirmedSignatureInfo[]).map((tx) => ({
        mint,
        tx,
      }))
    )
    .filter(({ tx }) => {
      if (sigSet.has(tx.signature)) return false;
      sigSet.add(tx.signature);
      return true;
    })
    .filter(({ tx }) => {
      return oldestSlot !== undefined && tx.slot >= oldestSlot;
    });

  if (mintAndTxs.length === 0) {
    if (fetching) {
      return <LoadingCard message="Loading history" />;
    } else if (failed) {
      return (
        <ErrorCard
          retry={() => fetchHistories(true)}
          text="Failed to fetch transaction history"
        />
      );
    }
    return (
      <ErrorCard
        retry={() => fetchHistories(true)}
        retryText="Try again"
        text="No transaction history found"
      />
    );
  }

  mintAndTxs.sort((a, b) => {
    if (a.tx.slot > b.tx.slot) return -1;
    if (a.tx.slot < b.tx.slot) return 1;
    return 0;
  });

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Token History</h3>
        <button
          className="btn btn-white btn-sm"
          disabled={fetching}
          onClick={() => fetchHistories(true)}
        >
          {fetching ? (
            <>
              <span className="spinner-grow spinner-grow-sm mr-2"></span>
              Loading
            </>
          ) : (
            <>
              <span className="fe fe-refresh-cw mr-2"></span>
              Refresh
            </>
          )}
        </button>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted w-1">Slot</th>
              <th className="text-muted">Result</th>
              <th className="text-muted">Token</th>
              <th className="text-muted">Instruction Type</th>
              <th className="text-muted">Transaction Signature</th>
            </tr>
          </thead>
          <tbody className="list">
            {mintAndTxs.map(({ mint, tx }) => (
              <TokenTransactionRow
                key={tx.signature}
                mint={mint}
                tx={tx}
                details={transactionDetailsCache[tx.signature]}
              />
            ))}
          </tbody>
        </table>
      </div>

      <div className="card-footer">
        {allFoundOldest ? (
          <div className="text-muted text-center">Fetched full history</div>
        ) : (
          <button
            className="btn btn-primary w-100"
            onClick={() => fetchHistories()}
            disabled={fetching}
          >
            {fetching ? (
              <>
                <span className="spinner-grow spinner-grow-sm mr-2"></span>
                Loading
              </>
            ) : (
              "Load More"
            )}
          </button>
        )}
      </div>
    </div>
  );
}

function instructionTypeName(
  ix: ParsedInstruction,
  tx: ConfirmedSignatureInfo
): string {
  try {
    const parsed = coerce(ix.parsed, ParsedInfo);
    const { type: rawType } = parsed;
    const type = coerce(rawType, TokenInstructionType);
    return IX_TITLES[type];
  } catch (err) {
    reportError(err, { signature: tx.signature });
    return "Unknown";
  }
}

const TokenTransactionRow = React.memo(
  ({
    mint,
    tx,
    details,
  }: {
    mint: PublicKey;
    tx: ConfirmedSignatureInfo;
    details: CacheEntry<Details> | undefined;
  }) => {
    const fetchDetails = useFetchTransactionDetails();
    const { cluster } = useCluster();

    // Fetch details on load
    React.useEffect(() => {
      if (!details) fetchDetails(tx.signature);
    }, []); // eslint-disable-line react-hooks/exhaustive-deps

    let statusText: string;
    let statusClass: string;
    if (tx.err) {
      statusClass = "warning";
      statusText = "Failed";
    } else {
      statusClass = "success";
      statusText = "Success";
    }

    const instructions =
      details?.data?.transaction?.transaction.message.instructions;
    if (!instructions)
      return (
        <tr key={tx.signature}>
          <td className="w-1">
            <Slot slot={tx.slot} link />
          </td>

          <td>
            <span className={`badge badge-soft-${statusClass}`}>
              {statusText}
            </span>
          </td>

          <td>
            <Address pubkey={mint} link truncate />
          </td>

          <td>
            <span className="spinner-grow spinner-grow-sm mr-2"></span>
            Loading
          </td>

          <td>
            <Signature signature={tx.signature} link />
          </td>
        </tr>
      );

    let tokenInstructionNames: InstructionType[] = [];

    if (details?.data?.transaction) {
      const transaction = details.data.transaction;

      tokenInstructionNames = instructions
        .map((ix, index): InstructionType | undefined => {
          let name = "Unknown";

          const innerInstructions: (
            | ParsedInstruction
            | PartiallyDecodedInstruction
          )[] = [];

          if (
            transaction.meta?.innerInstructions &&
            (cluster !== Cluster.MainnetBeta ||
              transaction.slot >= INNER_INSTRUCTIONS_START_SLOT)
          ) {
            transaction.meta.innerInstructions.forEach((ix) => {
              if (ix.index === index) {
                ix.instructions.forEach((inner) => {
                  innerInstructions.push(inner);
                });
              }
            });
          }

          let transactionInstruction;
          if (transaction?.transaction) {
            transactionInstruction = intoTransactionInstruction(
              transaction.transaction,
              ix
            );
          }

          if ("parsed" in ix) {
            if (ix.program === "spl-token") {
              name = instructionTypeName(ix, tx);
            } else {
              return undefined;
            }
          } else if (
            transactionInstruction &&
            isSerumInstruction(transactionInstruction)
          ) {
            try {
              name = parseSerumInstructionTitle(transactionInstruction);
            } catch (error) {
              reportError(error, { signature: tx.signature });
              return undefined;
            }
          } else if (
            transactionInstruction &&
            isTokenSwapInstruction(transactionInstruction)
          ) {
            try {
              name = parseTokenSwapInstructionTitle(transactionInstruction);
            } catch (error) {
              reportError(error, { signature: tx.signature });
              return undefined;
            }
          } else {
            if (
              ix.accounts.findIndex((account) =>
                account.equals(TOKEN_PROGRAM_ID)
              ) >= 0
            ) {
              name = "Unknown (Inner)";
            } else {
              return undefined;
            }
          }

          return {
            name: name,
            innerInstructions: innerInstructions,
          };
        })
        .filter((name) => name !== undefined) as InstructionType[];
    }

    return (
      <>
        {tokenInstructionNames.map((instructionType, index) => {
          return (
            <tr key={index}>
              <td className="w-1">
                <Slot slot={tx.slot} link />
              </td>

              <td>
                <span className={`badge badge-soft-${statusClass}`}>
                  {statusText}
                </span>
              </td>

              <td className="forced-truncate">
                <Address pubkey={mint} link truncateUnknown />
              </td>

              <td>
                <InstructionDetails instructionType={instructionType} tx={tx} />
              </td>

              <td className="forced-truncate">
                <Signature signature={tx.signature} link truncate />
              </td>
            </tr>
          );
        })}
      </>
    );
  }
);

function InstructionDetails({
  instructionType,
  tx,
}: {
  instructionType: InstructionType;
  tx: ConfirmedSignatureInfo;
}) {
  const [expanded, setExpanded] = React.useState(false);

  let instructionTypes = instructionType.innerInstructions
    .map((ix) => {
      if ("parsed" in ix && ix.program === "spl-token") {
        return instructionTypeName(ix, tx);
      }
      return undefined;
    })
    .filter((type) => type !== undefined);

  return (
    <>
      <p className="tree">
        {instructionTypes.length > 0 && (
          <span
            onClick={(e) => {
              e.preventDefault();
              setExpanded(!expanded);
            }}
            className={`c-pointer fe mr-2 ${
              expanded ? "fe-minus-square" : "fe-plus-square"
            }`}
          ></span>
        )}
        {instructionType.name}
      </p>
      {expanded && (
        <ul className="tree">
          {instructionTypes.map((type, index) => {
            return <li key={index}>{type}</li>;
          })}
        </ul>
      )}
    </>
  );
}
