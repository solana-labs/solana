import React from "react";
import {
  useFetchGossipNodes,
  useGossipNodes,
  Status,
  GossipNode,
} from "providers/gossipNodes";
import { ErrorCard } from "./common/ErrorCard";
import { LoadingCard } from "./common/LoadingCard";
import { PublicKey } from "@solana/web3.js";
import { Address } from "./common/Address";

const PAGE_SIZE = 25;

export function GossipNodesCard() {
  const [numDisplayed, setNumDisplayed] = React.useState(PAGE_SIZE);
  const gossipNodes = useGossipNodes();
  const fetchGossipNodes = useFetchGossipNodes();

  // Fetch supply on load
  React.useEffect(() => {
    if (gossipNodes === Status.Idle) fetchGossipNodes();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (gossipNodes === Status.Disconnected) {
    return <ErrorCard text="Not connected to the cluster" />;
  }

  if (gossipNodes === Status.Idle || gossipNodes === Status.Connecting)
    return <LoadingCard />;

  if (typeof gossipNodes === "string") {
    return <ErrorCard text={gossipNodes} retry={fetchGossipNodes} />;
  }

  return (
    <div className="card">
      {" "}
      <div className="card-header">
        <div className="row align-items-center">
          <div className="col">
            <h4 className="card-header-title">Network Nodes</h4>
          </div>

          <div className="col-auto">Total Nodes: {gossipNodes.length}</div>
        </div>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">#</th>
              <th className="text-muted">IDENTITY</th>
              <th className="text-muted text-end">IP</th>
              <th className="text-muted text-end">Gossip</th>
              <th className="text-muted text-end">TPU</th>
              <th className="text-muted text-end">Version</th>
            </tr>
          </thead>
          <tbody className="list">
            {gossipNodes
              .slice(0, numDisplayed)
              .map((el, index) => renderNetworkNodeRow(el, index))}
          </tbody>
        </table>
      </div>
      {gossipNodes.length > numDisplayed && (
        <div className="card-footer">
          <button
            className="btn btn-primary w-100"
            onClick={() =>
              setNumDisplayed((displayed) => displayed + PAGE_SIZE)
            }
          >
            Load More
          </button>
        </div>
      )}
    </div>
  );
}

const renderNetworkNodeRow = (node: GossipNode, index: number) => {
  const pubkey = new PublicKey(node.pubKey);

  return (
    <tr key={index}>
      <td>
        <span className="badge bg-gray-soft badge-pill">{index + 1}</span>
      </td>
      <td className="text-start">
        <Address pubkey={pubkey} link />
      </td>
      <td className="text-end">{node.ip}</td>
      <td className="text-end">{node.gossip ? node.gossip : <NullBadge />}</td>
      <td className="text-end">{node.tpu ? node.tpu : <NullBadge />}</td>
      <td className="text-end">{node.version}</td>
    </tr>
  );
};

const NullBadge = () => {
  return (
    <span className={`badge bg-secondary-soft`}>
      null
    </span>
  );
};
