import React from "react";

import { Account } from "providers/accounts";
import { useCluster } from "providers/cluster";
import { Program, Provider } from "@project-serum/anchor";
import { Connection, Keypair } from "@solana/web3.js";

import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";

import ReactJson from "react-json-view";

export function AnchorProgramCard({ program }: { program: Program }) {
  const { url } = useCluster();

  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Anchor IDL</h3>
            </div>
          </div>
        </div>

        <div className="card metadata-json-viewer m-4">
          <ReactJson
            src={program.idl}
            theme={"solarized"}
            style={{ padding: 25 }}
          />
        </div>
      </div>
    </>
  );
}
