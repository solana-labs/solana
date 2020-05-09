import React from "react";
import {
  StakeInstruction,
  TransactionInstruction,
  SignatureResult
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InitializeDetailsCard } from "./InitializeDetailsCard";
import { DelegateDetailsCard } from "./DelegateDetailsCard";
import { AuthorizeDetailsCard } from "./AuthorizeDetailsCard";
import { SplitDetailsCard } from "./SplitDetailsCard";
import { WithdrawDetailsCard } from "./WithdrawDetailsCard";
import { DeactivateDetailsCard } from "./DeactivateDetailsCard";

type DetailsProps = {
  ix: TransactionInstruction;
  result: SignatureResult;
  index: number;
};

export function StakeDetailsCard(props: DetailsProps) {
  let stakeInstructionType;
  try {
    stakeInstructionType = StakeInstruction.decodeInstructionType(props.ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  switch (stakeInstructionType) {
    case "Initialize":
      return <InitializeDetailsCard {...props} />;
    case "Delegate":
      return <DelegateDetailsCard {...props} />;
    case "Authorize":
      return <AuthorizeDetailsCard {...props} />;
    case "Split":
      return <SplitDetailsCard {...props} />;
    case "Withdraw":
      return <WithdrawDetailsCard {...props} />;
    case "Deactivate":
      return <DeactivateDetailsCard {...props} />;
    default:
      return <UnknownDetailsCard {...props} />;
  }
}
