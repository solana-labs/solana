import { Address } from "components/common/Address";
import { ExpandableRow } from "components/common/ExpandableRow";
import { Fragment, ReactNode } from "react";

export function mapToDisplayableFields(account: Object) {
  console.log(account);

  return Object.entries(account).map(([key, value]) => {
    try {
      console.log(key, value);
      return mapField(key, value);
    } catch (error: any) {
      console.log("Error while decoding IDL-based account data", error);
      return (
        <tr key={key}>
          <td>{key}</td>
          <td className="text-lg-end">
            <div>Failed to display data</div>{" "}
            {/* TODO: Check if data can be shown in hex format */}
          </td>
        </tr>
      );
    }
  });
}

function mapField(
  key: string,
  value: any,
  keySuffix?: any,
  nestingLevel: number = 0
): ReactNode {
  if (value === null) {
    return (
      <tr key={keySuffix ? `${key}-${keySuffix}` : key}>
        <td>{key}</td>
        <td className="text-lg-end">null</td>
      </tr>
    );
  }

  const objectType = value.constructor.name;
  console.log({ nestingLevel, key, objectType });

  let itemKey: string;
  if (nestingLevel === 0) {
    itemKey = key;
  } else if (keySuffix === null) {
    itemKey = key;
  } else if (/^-?\d+$/.test(keySuffix)) {
    itemKey = `#${keySuffix}`;
  } else {
    itemKey = keySuffix;
  }

  switch (objectType) {
    case "PublicKey":
      return (
        <tr
          key={keySuffix ? `${key}-${keySuffix}` : key}
          style={{
            ...(nestingLevel === 0 ? {} : { backgroundColor: "#141816" }),
          }}
        >
          <td
            style={{
              display: "flex",
              flexDirection: "row",
            }}
          >
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
          <td className="text-lg-end">
            <Address pubkey={value} link alignRight />
          </td>
        </tr>
      );

    case "Array":
      return (
        <ExpandableRow
          fieldName={key}
          nestingLevel={nestingLevel}
          key={keySuffix ? `${key}-${keySuffix}` : key}
        >
          <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
            {(value as any[]).map((item, i) =>
              mapField(key, item, i, nestingLevel + 1)
            )}
          </Fragment>
        </ExpandableRow>
      );

    case "Object":
      return (
        <ExpandableRow
          fieldName={key}
          nestingLevel={nestingLevel}
          key={keySuffix ? `${key}-${keySuffix}` : key}
        >
          <Fragment key={keySuffix ? `${key}-${keySuffix}` : key}>
            {Object.entries(value).map(([key, value]) =>
              mapField(key, value, key, nestingLevel + 1)
            )}
          </Fragment>
        </ExpandableRow>
      );

    default:
      return (
        <tr
          key={keySuffix ? `${key}-${keySuffix}` : key}
          style={{
            ...(nestingLevel === 0 ? {} : { backgroundColor: "#141816" }),
          }}
        >
          <td
            style={{
              display: "flex",
              flexDirection: "row",
            }}
          >
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
          <td className="text-lg-end">
            <div>{value.toString()}</div>
          </td>
        </tr>
      );
  }
}
