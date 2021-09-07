import React from "react";
import {
  SignatureResult,
  PublicKey,
  TransactionInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { reportError } from "utils/sentry";
import {GatewayInstruction, GatewayTokenState} from "@identity.com/solana-gateway-ts";

type DetailsProps = {
  ix: TransactionInstruction;
  result: SignatureResult;
  index: number;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

enum GatewayInstructionType {
  AddGatekeeper,
  Issue,
  SetState,
  UpdateExpiry,
  CloseGatekeeper,
}

const parseGatewayInstructionCode = (ix: TransactionInstruction): GatewayInstructionType => ix.data.slice(0, 1).readUInt8(0);

const codeToTitle = (code: GatewayInstructionType) => {
  switch (code) {
    case GatewayInstructionType.AddGatekeeper: return "Add Gatekeeper";
    case GatewayInstructionType.Issue: return "Issue";
    case GatewayInstructionType.SetState: return "Set State";
    case GatewayInstructionType.UpdateExpiry: return "Update Expiry";
    case GatewayInstructionType.CloseGatekeeper: return "Close Gatekeeper";
  }
}

// Converts a Borsh-deserialised "Enum" (an object with one populated
// property and the rest undefined) to a string
const stateToString = (state: GatewayTokenState | undefined) => {
  if (state?.frozen) return "FROZEN"
  if (state?.revoked) return "REVOKED"
  return "ACTIVE"
}

function parseGatewayInstruction(code: GatewayInstructionType, ix: TransactionInstruction) {
  const { issueVanilla, updateExpiry, setState } = GatewayInstruction.decode<GatewayInstruction>(ix.data);
  switch (code) {
    case GatewayInstructionType.Issue: 
      return { 
      payer: ix.keys[0].pubkey,
      gatewayToken: ix.keys[1].pubkey,
      owner: ix.keys[2].pubkey,
      gatekeeperAccount: ix.keys[3].pubkey,
      gatekeeperAuthority: ix.keys[4].pubkey,
      gatekeeperNetwork: ix.keys[5].pubkey,
      rent: ix.keys[6].pubkey,
      systemProgram: ix.keys[7].pubkey,
      seed: issueVanilla?.seed,
        // @ts-ignore
      expireTime: new Date(issueVanilla?.expireTime?.toNumber() * 1000).toISOString()
    };
    case GatewayInstructionType.UpdateExpiry:
      return {
        gatewayToken: ix.keys[0].pubkey,
        gatekeeperAuthority: ix.keys[1].pubkey,
        gatekeeperAccount: ix.keys[2].pubkey,
        expireTime: updateExpiry?.expireTime ? new Date(updateExpiry?.expireTime * 1000).toISOString() : "-"
      };
    case GatewayInstructionType.SetState:
      return {
        gatewayToken: ix.keys[0].pubkey,
        gatekeeperAuthority: ix.keys[1].pubkey,
        gatekeeperAccount: ix.keys[2].pubkey,
        state: stateToString(setState?.state)
      }
  }
}

export function GatewayTokenDetailsCard(props: DetailsProps) {
  try {
    const code = parseGatewayInstructionCode(props.ix);
    const title = codeToTitle(code)
    const created = parseGatewayInstruction(code, props.ix);
    console.log({code, title, created, data: props.ix.data.toJSON()});
    return <GatewayTokenInstruction title={title} info={created} {...props} />;
  } catch (err) {
    reportError(err, {});
    return <UnknownDetailsCard {...props} />;
  }
}

type InfoProps = {
  ix: TransactionInstruction;
  info: any;
  result: SignatureResult;
  index: number;
  title: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

function GatewayTokenInstruction(props: InfoProps) {
  const attributes: JSX.Element[] = [];

  for (let key in props.info) {
    let value = props.info[key];
    if (value === undefined) continue;

    // Flatten lists of public keys
    if (Array.isArray(value) && value.every((v) => v instanceof PublicKey)) {
      for (let i = 0; i < value.length; i++) {
        let publicKey = value[i];
        let label = `${key.charAt(0).toUpperCase() + key.slice(1)} - #${i + 1}`;

        attributes.push(
          <tr key={key + i}>
            <td>{label}</td>
            <td className="text-lg-right">
              <Address pubkey={publicKey} alignRight link />
            </td>
          </tr>
        );
      }
      continue;
    }
    
    let tag;
    let labelSuffix = "";
    if (value instanceof PublicKey) {
      tag = <Address pubkey={value} alignRight link />;
    } else {
      tag = <>{value}</>;
    }

    let label = key.charAt(0).toUpperCase() + key.slice(1) + labelSuffix;

    attributes.push(
      <tr key={key}>
        <td>{label}</td>
        <td className="text-lg-right">{tag}</td>
      </tr>
    );
  }

  return (
    <InstructionCard
      ix={props.ix}
      index={props.index}
      result={props.result}
      title={props.title}
      innerCards={props.innerCards}
      childIndex={props.childIndex}
    >
      {attributes}
    </InstructionCard>
  );
}
