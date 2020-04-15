import React from "react";

type SignatureProps = {
  text: string
}

function Signature({ text }: SignatureProps) {
  const copyToClipboard = () => navigator.clipboard.writeText(text);
  // TODO: how to make onClick pop up a toast or other notification?

  return <code onClick={copyToClipboard}>{text}</code>;
}

export default Signature;
