import React from "react";
import {
  SignatureResult,
  ParsedTransaction,
  ParsedInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InitializeDetailsCard } from "./InitializeDetailsCard";
import { DelegateDetailsCard } from "./DelegateDetailsCard";
import { AuthorizeDetailsCard } from "./AuthorizeDetailsCard";
import { SplitDetailsCard } from "./SplitDetailsCard";
import { WithdrawDetailsCard } from "./WithdrawDetailsCard";
import { DeactivateDetailsCard } from "./DeactivateDetailsCard";
import { ParsedInfo } from "validators";
import { reportError } from "utils/sentry";
import { coerce } from "superstruct";
import { IX_STRUCTS } from "./types";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
};

export function StakeDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    const info = coerce(parsed.info, IX_STRUCTS[parsed.type]);

    switch (parsed.type) {
      case "initialize":
        return <InitializeDetailsCard info={info} {...props} />;
      case "delegate":
        return <DelegateDetailsCard info={info} {...props} />;
      case "authorize":
        return <AuthorizeDetailsCard info={info} {...props} />;
      case "split":
        return <SplitDetailsCard info={info} {...props} />;
      case "withdraw":
        return <WithdrawDetailsCard info={info} {...props} />;
      case "deactivate":
        return <DeactivateDetailsCard info={info} {...props} />;
      default:
        return <UnknownDetailsCard {...props} />;
    }
  } catch (error) {
    reportError(error, {
      signature: props.tx.signatures[0],
    });
    return <UnknownDetailsCard {...props} />;
  }
}
