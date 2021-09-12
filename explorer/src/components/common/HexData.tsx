import React, { ReactNode } from "react";
import { Buffer } from "buffer";
import { Copyable } from "./Copyable";

export function HexData({ raw }: { raw: Buffer }) {
  if (!raw) {
    return <span>No data</span>;
  }

  const chunks = [];
  const hexString = raw.toString("hex");
  for (let i = 0; i < hexString.length; i += 2) {
    chunks.push(hexString.slice(i, i + 2));
  }

  const spans: ReactNode[] = [];
  for (let i = 0; i < chunks.length; i += 4) {
    const color = i % 8 === 0 ? "text-white" : "text-gray-500";
    spans.push(
      <span key={i} className={color}>
        {chunks.slice(i, i + 4).join(" ")}&emsp;
      </span>
    );
  }

  function Content() {
    return (
      <Copyable text={hexString}>
        <pre className="d-inline-block text-left mb-0 data-wrap">{spans}</pre>
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
