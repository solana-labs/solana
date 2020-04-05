import React from "react";
import { useCluster, ClusterStatus, Cluster } from "../providers/cluster";

function ClusterStatusButton({
  onClick,
  expand
}: {
  onClick: () => void;
  expand?: boolean;
}) {
  return (
    <div onClick={onClick}>
      <Button expand={expand} />
    </div>
  );
}

function Button({ expand }: { expand?: boolean }) {
  const { status, cluster, name, customUrl } = useCluster();
  const statusName = cluster !== Cluster.Custom ? `${name}` : `${customUrl}`;

  const btnClasses = (variant: string) => {
    if (expand) {
      return `btn lift d-block btn-${variant}`;
    } else {
      return `btn lift btn-outline-${variant}`;
    }
  };

  let spinnerClasses = "spinner-grow spinner-grow-sm mr-2";
  if (!expand) {
    spinnerClasses += " text-warning";
  }

  switch (status) {
    case ClusterStatus.Connected:
      return (
        <span className={btnClasses("success")}>
          <span className="fe fe-check-circle mr-2"></span>
          {statusName}
        </span>
      );

    case ClusterStatus.Connecting:
      return (
        <span className={btnClasses("warning")}>
          <span
            className={spinnerClasses}
            role="status"
            aria-hidden="true"
          ></span>
          {statusName}
        </span>
      );

    case ClusterStatus.Failure:
      return (
        <span className={btnClasses("danger")}>
          <span className="fe fe-alert-circle mr-2"></span>
          {statusName}
        </span>
      );
  }
}

export default ClusterStatusButton;
