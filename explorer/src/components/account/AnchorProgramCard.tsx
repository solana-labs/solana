import dynamic from "next/dynamic";
import { PublicKey } from "@solana/web3.js";
import { useAnchorProgram } from "src/providers/anchor";
import { useCluster } from "src/providers/cluster";
const ReactJson = dynamic(() => import("react-json-view"), { ssr: false });

export function AnchorProgramCard({ programId }: { programId: PublicKey }) {
  const { url } = useCluster();
  const program = useAnchorProgram(programId.toString(), url);

  if (!program) {
    return null;
  }

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
            collapsed={1}
          />
        </div>
      </div>
    </>
  );
}
