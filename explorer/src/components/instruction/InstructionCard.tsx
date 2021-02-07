import React, { useContext } from "react";
import {
  TransactionInstruction,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { RawDetails } from "./RawDetails";
import { RawParsedDetails } from "./RawParsedDetails";
import { SignatureContext } from "../../pages/TransactionDetailsPage";
import {
  useTransactionDetails,
  useFetchRawTransaction,
} from "providers/transactions/details";
import { Address } from "components/common/Address";

type InstructionProps = {
  title: string;
  children?: React.ReactNode;
  result: SignatureResult;
  index: number;
  ix: TransactionInstruction | ParsedInstruction;
  defaultRaw?: boolean;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function InstructionCard({
  title,
  children,
  result,
  index,
  ix,
  defaultRaw,
  innerCards,
  childIndex,
}: InstructionProps) {
  const [resultClass] = ixResult(result, index);
  const [showRaw, setShowRaw] = React.useState(defaultRaw || false);
  const signature = useContext(SignatureContext);
  const details = useTransactionDetails(signature);
  let raw: TransactionInstruction | undefined = undefined;
  if (details && childIndex === undefined) {
    raw = details?.data?.raw?.transaction.instructions[index];
  }
  const fetchRaw = useFetchRawTransaction();
  const fetchRawTrigger = () => fetchRaw(signature);

  const rawClickHandler = () => {
    if (!defaultRaw && !showRaw && !raw) {
      fetchRawTrigger();
    }

    return setShowRaw((r) => !r);
  };

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className={`badge badge-soft-${resultClass} mr-2`}>
            #{index + 1}
            {childIndex !== undefined ? `.${childIndex + 1}` : ""}
          </span>
          {title}
        </h3>

        <button
          disabled={defaultRaw}
          className={`btn btn-sm d-flex ${
            showRaw ? "btn-black active" : "btn-white"
          }`}
          onClick={rawClickHandler}
        >
          <span className="fe fe-code mr-1"></span>
          Raw
        </button>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <tbody className="list">
            {showRaw ? (
              <>
                <tr>
                  <td>Program</td>
                  <td className="text-lg-right">
                    <Address pubkey={ix.programId} alignRight link />
                  </td>
                </tr>
                {"parsed" in ix ? (
                  <RawParsedDetails ix={ix}>
                    {raw ? <RawDetails ix={raw} /> : null}
                  </RawParsedDetails>
                ) : (
                  <RawDetails ix={ix} />
                )}
              </>
            ) : (
              children
            )}
            {innerCards && innerCards.length > 0 && (
              <tr>
                <td colSpan={2}>
                  Inner Instructions
                  <div className="inner-cards">{innerCards}</div>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function ixResult(result: SignatureResult, index: number) {
  if (result.err) {
    const err = result.err as any;
    const ixError = err["InstructionError"];
    if (ixError && Array.isArray(ixError)) {
      const [errorIndex, error] = ixError;
      if (Number.isInteger(errorIndex) && errorIndex === index) {
        return ["warning", `Error: ${JSON.stringify(error)}`];
      }
    }
    return ["dark"];
  }
  return ["success"];
}
