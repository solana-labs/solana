import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { RawDetails } from "./RawDetails";

type InstructionProps = {
  title: string;
  children?: React.ReactNode;
  result: SignatureResult;
  index: number;
  ix: TransactionInstruction;
  defaultRaw?: boolean;
};

export function InstructionCard({
  title,
  children,
  result,
  index,
  ix,
  defaultRaw
}: InstructionProps) {
  const [resultClass] = ixResult(result, index);
  const [showRaw, setShowRaw] = React.useState(defaultRaw || false);

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          <span className={`badge badge-soft-${resultClass} mr-2`}>
            #{index + 1}
          </span>
          {title}
        </h3>

        <button
          className={`btn btn-sm d-flex ${
            showRaw ? "btn-dark active" : "btn-white"
          }`}
          onClick={() => setShowRaw(r => !r)}
        >
          <span className="fe fe-code mr-1"></span>
          Raw
        </button>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <tbody className="list">
            {showRaw ? <RawDetails ix={ix} /> : children}
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
