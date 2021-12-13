import React from "react";
import { ConfirmedSignatureInfo } from "@solana/web3.js";
import {
  getTokenProgramInstructionName,
  InstructionType,
} from "utils/instruction";

export function InstructionDetails({
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
        return getTokenProgramInstructionName(ix, tx);
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
            className={`c-pointer fe me-2 ${
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
