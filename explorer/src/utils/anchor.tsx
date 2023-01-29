import React, { Fragment, ReactNode, useState } from "react";
import { Cluster } from "providers/cluster";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { BorshInstructionCoder, Program, Idl } from "@project-serum/anchor";
import { useAnchorProgram } from "providers/anchor";
import { getProgramName } from "utils/tx";
import { snakeToTitleCase, camelToTitleCase, numberWithSeparator } from "utils";
import {
  IdlField,
  IdlInstruction,
  IdlType,
  IdlTypeDef,
} from "@project-serum/anchor/dist/cjs/idl";
import { Address } from "components/common/Address";
import ReactJson from "react-json-view";

export function getAnchorProgramName(
  program: Program | null
): string | undefined {
  return program ? snakeToTitleCase(program.idl.name) : undefined;
}

export function AnchorProgramName({
  programId,
  url,
  defaultName = "Unknown Program",
}: {
  programId: PublicKey;
  url: string;
  defaultName?: string;
}) {
  const program = useAnchorProgram(programId.toString(), url);
  const programName = getAnchorProgramName(program) || defaultName;
  return <>{programName}</>;
}

export function ProgramName({
  programId,
  cluster,
  url,
}: {
  programId: PublicKey;
  cluster: Cluster;
  url: string;
}) {
  const defaultProgramName = getProgramName(programId.toBase58(), cluster);
  return (
    <React.Suspense fallback={<>{defaultProgramName}</>}>
      <AnchorProgramName
        programId={programId}
        url={url}
        defaultName={defaultProgramName}
      />
    </React.Suspense>
  );
}

export function getAnchorNameForInstruction(
  ix: TransactionInstruction,
  program: Program
): string | null {
  const coder = new BorshInstructionCoder(program.idl);
  const decodedIx = coder.decode(ix.data);

  if (!decodedIx) {
    return null;
  }

  var _ixTitle = decodedIx.name;
  return _ixTitle.charAt(0).toUpperCase() + _ixTitle.slice(1);
}

export function getAnchorAccountsFromInstruction(
  decodedIx: Object | null,
  program: Program
):
  | {
      name: string;
      isMut: boolean;
      isSigner: boolean;
      pda?: Object;
    }[]
  | null {
  if (decodedIx) {
    // get ix accounts
    const idlInstructions = program.idl.instructions.filter(
      // @ts-ignore
      (ix) => ix.name === decodedIx.name
    );
    if (idlInstructions.length === 0) {
      return null;
    }
    return idlInstructions[0].accounts as {
      // type coercing since anchor doesn't export the underlying type
      name: string;
      isMut: boolean;
      isSigner: boolean;
      pda?: Object;
    }[];
  }
  return null;
}

export function mapIxArgsToRows(ixArgs: any, ixType: IdlInstruction, idl: Idl) {
  return Object.entries(ixArgs).map(([key, value]) => {
    try {
      const fieldDef = ixType.args.find((ixDefArg) => ixDefArg.name === key);
      if (!fieldDef) {
        throw Error(
          `Could not find expected ${key} field on account type definition for ${ixType.name}`
        );
      }
      return mapField(key, value, fieldDef.type, idl);
    } catch (error: any) {
      console.log("Error while displaying IDL-based account data", error);
      return (
        <tr key={key}>
          <td>{key}</td>
          <td className="text-lg-end">
            <td className="metadata-json-viewer m-4">
              <ReactJson src={ixArgs} theme="solarized" />
            </td>
          </td>
        </tr>
      );
    }
  });
}

export function mapAccountToRows(
  accountData: any,
  accountType: IdlTypeDef,
  idl: Idl
) {
  return Object.entries(accountData).map(([key, value]) => {
    try {
      if (accountType.type.kind !== "struct") {
        throw Error(
          `Account ${accountType.name} is of type ${accountType.type.kind} (expected: 'struct')`
        );
      }
      const fieldDef = accountType.type.fields.find(
        (ixDefArg) => ixDefArg.name === key
      );
      if (!fieldDef) {
        throw Error(
          `Could not find expected ${key} field on account type definition for ${accountType.name}`
        );
      }
      return mapField(key, value as any, fieldDef.type, idl);
    } catch (error: any) {
      console.log("Error while displaying IDL-based account data", error);
      return (
        <tr key={key}>
          <td>{key}</td>
          <td className="text-lg-end">
            <td className="metadata-json-viewer m-4">
              <ReactJson src={accountData} theme="solarized" />
            </td>
          </td>
        </tr>
      );
    }
  });
}

function mapField(
  key: string,
  value: any,
  type: IdlType,
  idl: Idl,
  keySuffix?: any,
  nestingLevel: number = 0
): ReactNode {
  let itemKey = key;
  if (/^-?\d+$/.test(keySuffix)) {
    itemKey = `#${keySuffix}`;
  }
  itemKey = camelToTitleCase(itemKey);

  if (value === undefined) {
    return (
      <SimpleRow
        key={keySuffix ? `${key}-${keySuffix}` : key}
        rawKey={key}
        type={type}
        keySuffix={keySuffix}
        nestingLevel={nestingLevel}
      >
        <div>null</div>
      </SimpleRow>
    );
  }

  if (
    type === "u8" ||
    type === "i8" ||
    type === "u16" ||
    type === "i16" ||
    type === "u32" ||
    type === "i32" ||
    type === "f32" ||
    type === "u64" ||
    type === "i64" ||
    type === "f64" ||
    type === "u128" ||
    type === "i128"
  ) {
    return (
      <SimpleRow
        key={keySuffix ? `${key}-${keySuffix}` : key}
        rawKey={key}
        type={type}
        keySuffix={keySuffix}
        nestingLevel={nestingLevel}
      >
        <div>{numberWithSeparator(value.toString())}</div>
      </SimpleRow>
    );
  } else if (type === "bool" || type === "bytes" || type === "string") {
    return (
      <SimpleRow
        key={keySuffix ? `${key}-${keySuffix}` : key}
        rawKey={key}
        type={type}
        keySuffix={keySuffix}
        nestingLevel={nestingLevel}
      >
        <div>{value.toString()}</div>
      </SimpleRow>
    );
  } else if (type === "publicKey") {
    return (
      <SimpleRow
        key={keySuffix ? `${key}-${keySuffix}` : key}
        rawKey={key}
        type={type}
        keySuffix={keySuffix}
        nestingLevel={nestingLevel}
      >
        <Address pubkey={value} link alignRight />
      </SimpleRow>
    );
  } else if ("defined" in type) {
    const fieldType = idl.types?.find((t) => t.name === type.defined);
    if (!fieldType) {
      throw Error(`Could not type definition for ${type.defined} field in IDL`);
    }
    if (fieldType.type.kind === "struct") {
      const structFields = fieldType.type.fields;
      return (
        <ExpandableRow
          fieldName={itemKey}
          fieldType={typeDisplayName(type)}
          nestingLevel={nestingLevel}
          key={keySuffix ? `${key}-${keySuffix}` : key}
        >
          <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
            {Object.entries(value).map(
              ([innerKey, innerValue]: [string, any]) => {
                const innerFieldType = structFields.find(
                  (t) => t.name === innerKey
                );
                if (!innerFieldType) {
                  throw Error(
                    `Could not type definition for ${innerKey} field in user-defined struct ${fieldType.name}`
                  );
                }
                return mapField(
                  innerKey,
                  innerValue,
                  innerFieldType?.type,
                  idl,
                  key,
                  nestingLevel + 1
                );
              }
            )}
          </Fragment>
        </ExpandableRow>
      );
    } else {
      const enumVariantName = Object.keys(value)[0];
      const variant = fieldType.type.variants.find(
        (val) =>
          val.name.toLocaleLowerCase() === enumVariantName.toLocaleLowerCase()
      );

      return variant && variant.fields ? (
        <ExpandableRow
          fieldName={itemKey}
          fieldType={typeDisplayName({ enum: enumVariantName })}
          nestingLevel={nestingLevel}
          key={keySuffix ? `${key}-${keySuffix}` : key}
        >
          <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
            {Object.entries(value[enumVariantName]).map(
              ([innerKey, innerValue]: [string, any], index) => {
                const innerFieldType = variant.fields![index];
                if (!innerFieldType) {
                  throw Error(
                    `Could not type definition for ${innerKey} field in user-defined struct ${fieldType.name}`
                  );
                }
                return mapField(
                  innerKey,
                  innerValue,
                  (innerFieldType as any).name
                    ? (innerFieldType as IdlField).type
                    : (innerFieldType as IdlType),
                  idl,
                  key,
                  nestingLevel + 1
                );
              }
            )}
          </Fragment>
        </ExpandableRow>
      ) : (
        <SimpleRow
          key={keySuffix ? `${key}-${keySuffix}` : key}
          rawKey={key}
          type={{ enum: type.defined }}
          keySuffix={keySuffix}
          nestingLevel={nestingLevel}
        >
          {camelToTitleCase(enumVariantName)}
        </SimpleRow>
      );
    }
  } else if ("option" in type) {
    if (value === null) {
      return (
        <SimpleRow
          key={keySuffix ? `${key}-${keySuffix}` : key}
          rawKey={key}
          type={type}
          keySuffix={keySuffix}
          nestingLevel={nestingLevel}
        >
          Not provided
        </SimpleRow>
      );
    }
    return mapField(key, value, type.option, idl, key, nestingLevel);
  } else if ("vec" in type) {
    const itemType = type.vec;
    return (
      <ExpandableRow
        fieldName={itemKey}
        fieldType={typeDisplayName(type)}
        nestingLevel={nestingLevel}
        key={keySuffix ? `${key}-${keySuffix}` : key}
      >
        <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
          {(value as any[]).map((item, i) =>
            mapField(key, item, itemType, idl, i, nestingLevel + 1)
          )}
        </Fragment>
      </ExpandableRow>
    );
  } else if ("array" in type) {
    const [itemType] = type.array;
    return (
      <ExpandableRow
        fieldName={itemKey}
        fieldType={typeDisplayName(type)}
        nestingLevel={nestingLevel}
        key={keySuffix ? `${key}-${keySuffix}` : key}
      >
        <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
          {(value as any[]).map((item, i) =>
            mapField(key, item, itemType, idl, i, nestingLevel + 1)
          )}
        </Fragment>
      </ExpandableRow>
    );
  } else {
    console.log("Impossible type:", type);
    return (
      <tr key={keySuffix ? `${key}-${keySuffix}` : key}>
        <td>{camelToTitleCase(key)}</td>
        <td></td>
        <td className="text-lg-end">???</td>
      </tr>
    );
  }
}

function SimpleRow({
  rawKey,
  type,
  keySuffix,
  nestingLevel = 0,
  children,
}: {
  rawKey: string;
  type: IdlType | { enum: string };
  keySuffix?: any;
  nestingLevel: number;
  children?: ReactNode;
}) {
  let itemKey = rawKey;
  if (/^-?\d+$/.test(keySuffix)) {
    itemKey = `#${keySuffix}`;
  }
  itemKey = camelToTitleCase(itemKey);
  return (
    <tr
      style={{
        ...(nestingLevel === 0 ? {} : { backgroundColor: "#141816" }),
      }}
    >
      <td className="d-flex flex-row">
        {nestingLevel > 0 && (
          <span
            className="text-info fe fe-corner-down-right me-2"
            style={{
              paddingLeft: `${15 * nestingLevel}px`,
            }}
          />
        )}
        <div>{itemKey}</div>
      </td>
      <td>{typeDisplayName(type)}</td>
      <td className="text-lg-end">{children}</td>
    </tr>
  );
}

export function ExpandableRow({
  fieldName,
  fieldType,
  nestingLevel,
  children,
}: {
  fieldName: string;
  fieldType: string;
  nestingLevel: number;
  children: React.ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <>
      <tr
        style={{
          ...(nestingLevel === 0 ? {} : { backgroundColor: "#141816" }),
        }}
      >
        <td className="d-flex flex-row">
          {nestingLevel > 0 && (
            <div
              className="text-info fe fe-corner-down-right me-2"
              style={{
                paddingLeft: `${15 * nestingLevel}px`,
              }}
            />
          )}
          <div>{fieldName}</div>
        </td>
        <td>{fieldType}</td>
        <td
          className="text-lg-end"
          onClick={() => setExpanded((current) => !current)}
        >
          <div className="c-pointer">
            {expanded ? (
              <>
                <span className="text-info me-2">Collapse</span>
                <span className="fe fe-chevron-up" />
              </>
            ) : (
              <>
                <span className="text-info me-2">Expand</span>
                <span className="fe fe-chevron-down" />
              </>
            )}
          </div>
        </td>
      </tr>
      {expanded && <>{children}</>}
    </>
  );
}

function typeDisplayName(
  type:
    | IdlType
    | {
        enum: string;
      }
): string {
  switch (type) {
    case "bool":
    case "u8":
    case "i8":
    case "u16":
    case "i16":
    case "u32":
    case "i32":
    case "f32":
    case "u64":
    case "i64":
    case "f64":
    case "u128":
    case "i128":
    case "bytes":
    case "string":
      return type.toString();
    case "publicKey":
      return "PublicKey";
    default:
      if ("enum" in type) return `${type.enum} (enum)`;
      if ("defined" in type) return type.defined;
      if ("option" in type) return `${typeDisplayName(type.option)} (optional)`;
      if ("vec" in type) return `${typeDisplayName(type.vec)}[]`;
      if ("array" in type)
        return `${typeDisplayName(type.array[0])}[${type.array[1]}]`;
      return "unkonwn";
  }
}
