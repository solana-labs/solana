import React, { useState, ReactNode } from "react";

type CopyableProps = {
  text: string;
  children: ReactNode;
};

const popover = (
  <div className="popover fade bs-popover-right show">
    <div className="arrow" />
    <div className="popover-body">Copied!</div>
  </div>
);

function Copyable({ text, children }: CopyableProps) {
  const [showPopover, setShowPopover] = useState(false);

  const copyToClipboard = () => navigator.clipboard.writeText(text);
  const handleClick = () =>
    copyToClipboard().then(() => {
      setShowPopover(true);
      setTimeout(setShowPopover.bind(null, false), 2500);
    });

  return (
    <div className="copyable">
      <div onClick={handleClick}>{children}</div>
      {showPopover && popover}
    </div>
  );
}

export default Copyable;
