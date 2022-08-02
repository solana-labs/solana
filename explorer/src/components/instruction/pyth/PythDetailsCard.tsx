import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import { InstructionCard } from "../InstructionCard";
import { PythInstruction } from "./program";
import UpdatePriceDetailsCard from "./UpdatePriceDetailsCard";
import BasePublisherOperationCard from "./BasePublisherOperationCard";
import AddProductDetailsCard from "./AddProductDetailsCard";
import AddPriceDetailsCard from "./AddPriceDetailsCard";
import UpdateProductDetailsCard from "./UpdateProductDetailsCard";
import InitMappingDetailsCard from "./InitMappingDetailsCard";
import AddMappingDetailsCard from "./AddMappingDetailsCard";
import AggregatePriceDetailsCard from "./AggregatePriceDetailsCard";
import InitPriceDetailsCard from "./InitPriceDetailsCard";

export function PythDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { url } = useCluster();
  const { ix, index, result, signature, innerCards, childIndex } = props;

  try {
    let ixType = PythInstruction.decodeInstructionType(ix);

    switch (ixType) {
      case "InitMapping":
        return (
          <InitMappingDetailsCard
            info={PythInstruction.decodeInitMapping(ix)}
            {...props}
          />
        );
      case "AddMapping":
        return (
          <AddMappingDetailsCard
            info={PythInstruction.decodeAddMapping(ix)}
            {...props}
          />
        );
      case "AddProduct":
        return (
          <AddProductDetailsCard
            info={PythInstruction.decodeAddProduct(ix)}
            {...props}
          />
        );
      case "UpdateProduct":
        return (
          <UpdateProductDetailsCard
            info={PythInstruction.decodeUpdateProduct(ix)}
            {...props}
          />
        );
      case "AddPrice":
        return (
          <AddPriceDetailsCard
            info={PythInstruction.decodeAddPrice(ix)}
            {...props}
          />
        );
      case "AddPublisher":
        return (
          <BasePublisherOperationCard
            operationName="Add Publisher"
            info={PythInstruction.decodeAddPublisher(ix)}
            {...props}
          />
        );
      case "DeletePublisher":
        return (
          <BasePublisherOperationCard
            operationName="Delete Publisher"
            info={PythInstruction.decodeDeletePublisher(ix)}
            {...props}
          />
        );
      case "UpdatePrice":
        return (
          <UpdatePriceDetailsCard
            info={PythInstruction.decodeUpdatePrice(ix)}
            {...props}
          />
        );

      case "UpdatePriceNoFailOnError":
        return (
          <UpdatePriceDetailsCard
            info={PythInstruction.decodeUpdatePriceNoFailOnError(ix)}
            {...props}
          />
        );
      case "AggregatePrice":
        return (
          <AggregatePriceDetailsCard
            info={PythInstruction.decodeAggregatePrice(ix)}
            {...props}
          />
        );
      case "InitPrice":
        return (
          <InitPriceDetailsCard
            info={PythInstruction.decodeInitPrice(ix)}
            {...props}
          />
        );
    }
  } catch (error) {
    reportError(error, {
      url: url,
      signature: signature,
    });
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`Pyth: Unknown Instruction`}
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
