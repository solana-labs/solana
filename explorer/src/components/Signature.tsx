import React, { useState } from "react";

type SignatureProps = {
  text: string;
};

const popover = (
  <div className="popover fade bs-popover-right show">
    <div className="arrow" />
    <div className="popover-body">Copied!</div>
  </div>
);

function Signature({ text }: SignatureProps) {
  const [showPopover, setShowPopover] = useState(false);

  const copyToClipboard = () => navigator.clipboard.writeText(text);
  const handleClick = () =>
    copyToClipboard().then(() => {
      setShowPopover(true);
      setTimeout(setShowPopover.bind(null, false), 2500);
    });

  return (
    <div className="signature">
      <code onClick={handleClick}>{text}</code>
      {showPopover && popover}
    </div>
  );
}

export default Signature;
