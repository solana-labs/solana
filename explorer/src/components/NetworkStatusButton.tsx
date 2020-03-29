import React from "react";
import { useNetwork, NetworkStatus, Network } from "../providers/network";

function NetworkStatusButton({ onClick }: { onClick: () => void }) {
  return (
    <div onClick={onClick}>
      <Button />
    </div>
  );
}

function Button() {
  const { status, network, name, customUrl } = useNetwork();
  const statusName = network !== Network.Custom ? `${name}` : `${customUrl}`;

  switch (status) {
    case NetworkStatus.Connected:
      return (
        <span className="btn btn-outline-success lift">
          <span className="fe fe-check-circle mr-2"></span>
          {statusName}
        </span>
      );

    case NetworkStatus.Connecting:
      return (
        <span className="btn btn-outline-warning lift">
          <span
            className="spinner-grow spinner-grow-sm text-warning mr-2"
            role="status"
            aria-hidden="true"
          ></span>
          {statusName}
        </span>
      );

    case NetworkStatus.Failure:
      return (
        <span className="btn btn-outline-danger lift">
          <span className="fe fe-alert-circle mr-2"></span>
          {statusName}
        </span>
      );
  }
}

export default NetworkStatusButton;
