import React, { ReactNode } from "react";
import { Buffer } from "buffer";
import { Copyable } from "./Copyable";

export function HexData({ raw }: { raw: Buffer }) {
  if (!raw || raw.length === 0) {
    return <span>No data</span>;
  }

  const chunks = [];
  const hexString = raw.toString("hex");
  for (let i = 0; i < hexString.length; i += 2) {
    chunks.push(hexString.slice(i, i + 2));
  }

  const SPAN_SIZE = 4;
  const ROW_SIZE = 4 * SPAN_SIZE;

  const divs: ReactNode[] = [];
  let spans: ReactNode[] = [];
  for (let i = 0; i < chunks.length; i += SPAN_SIZE) {
    const color = i % (2 * SPAN_SIZE) === 0 ? "text-white" : "text-gray-500";
    spans.push(
      <span key={i} className={color}>
        {chunks.slice(i, i + SPAN_SIZE).join(" ")}&emsp;
      </span>
    );

    if (
      i % ROW_SIZE === ROW_SIZE - SPAN_SIZE ||
      i >= chunks.length - SPAN_SIZE
    ) {
      divs.push(<div key={i / ROW_SIZE}>{spans}</div>);

      // clear spans
      spans = [];
    }
  }

  function Content() {
    return (
      <Copyable text={hexString}>
        <pre className="d-inline-block text-start mb-0">{divs}</pre>
      </Copyable>
    );
  }

  return (
    <>
      <div className="d-none d-lg-flex align-items-center justify-content-end">
        <Content />
      </div>
      <div className="d-flex d-lg-none align-items-center">
        <Content />
      </div>
    </>
  );
}
