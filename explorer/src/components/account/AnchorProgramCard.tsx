import React from "react";

import {
  Account,
} from "providers/accounts";
import { useCluster } from "providers/cluster";
import { Program, Provider, Wallet, AccountsCoder} from "@project-serum/anchor";
import { Connection, PublicKey, Keypair } from "@solana/web3.js";

import ReactJson from "react-json-view";

export function AnchorProgramCard({ account }: { account: Account }) {
    const { url } = useCluster();
    const [decodedAnchorAccountData, setDecodedAnchorAccountData] = React.useState<Object>();

    React.useEffect(()=>{
        (async()=>{
            const connection = new Connection(url);
            const provider = new Provider(connection, new Wallet(Keypair.generate()), {
                skipPreflight: false,
                commitment: 'confirmed',
                preflightCommitment: 'confirmed'
            })

            await Program.at(account.pubkey, provider)
            .then(program=>{
                setDecodedAnchorAccountData(program.idl)
            })
            .catch(error=>{
                console.log('No IDL found for address. Why did the tab load?',  error)
            });
        })()
    },[account, url])

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
                src={decodedAnchorAccountData!}
                theme={"solarized"}
                style={{ padding: 25 }}
            />
            </div>
        </div>
        </>
    );
}