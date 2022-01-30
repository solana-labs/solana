import React, { useState } from "react";

export function ExpandableRow({
  fieldName,
  nestingLevel,
  children,
}: {
  fieldName: string;
  nestingLevel: number;
  children: React.ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <>
      <tr
        key={fieldName}
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
            <div
              className="text-info fe fe-corner-down-right me-2"
              style={{
                paddingLeft: `${15 * nestingLevel}px`,
              }}
            />
          )}
          <div>{fieldName}</div>
        </td>
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
