import React from "react";

import {
  Account,
} from "providers/accounts";
import { useCluster } from "providers/cluster";
import { Address } from "components/common/Address";
import { Program, Provider, Wallet, AccountsCoder} from "@project-serum/anchor";
import { Connection, PublicKey, Keypair } from "@solana/web3.js";

export function AnchorAccountCard({ account }: { account: Account }) {
  const { url } = useCluster();
  const [decodedAnchorAccountData, setDecodedAnchorAccountData] = React.useState<any>()
  const [decodedAnchorAccountName, setDecodedAnchorAccountName] = React.useState<string>()

  React.useEffect(()=>{
    setDecodedAnchorAccountData(undefined);
    (async()=>{
      const connection = new Connection(url);
      const provider = new Provider(connection, new Wallet(Keypair.generate()), {
        skipPreflight: false,
        commitment: 'confirmed',
        preflightCommitment: 'confirmed'
      })

      if (!account.details) {
        return
      }

      // TODO this should come from somewhere inside the client already? This lookup seems bad.
      // Appears we don't cache data for accounts we don't know the type of?
      const accountInfo = await connection.getAccountInfo(account.pubkey)
      const discriminator = accountInfo?.data.slice(0, 8)
      if (!discriminator){
        return
      }

      await Program.at(account.details.owner, provider)
      .then(program=>{
        if (program && accountInfo && accountInfo.data){
          // Iterate all the structs, see if any of the name-hashes match
          Object.keys(program.account).forEach(accountType =>{
            const layoutName = capitalizeFirstLetter(accountType)
            const discriminatorToCheck = AccountsCoder.accountDiscriminator(layoutName)

            if (equal(discriminatorToCheck, discriminator)) {
              const decodedAnchorObject = program.account[accountType].coder.accounts.decode(layoutName, accountInfo.data)
              setDecodedAnchorAccountName(layoutName)
              setDecodedAnchorAccountData(decodedAnchorObject);
            }
          })
        }
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
              <h3 className="card-header-title">{decodedAnchorAccountName}</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Key</th>
                <th className="text-muted">Value</th>
              </tr>
            </thead>
            <tbody className="list">
              {decodedAnchorAccountData && Object.keys(decodedAnchorAccountData).map((key)=>{
                return renderAccountRow(key, decodedAnchorAccountData[key]);
              })}
            </tbody>
          </table>
        </div>
        <div className="card-footer">
          <div className="text-muted text-center">
            {decodedAnchorAccountData && Object.keys(decodedAnchorAccountData).length > 0
            ?
              `Decoded ${Object.keys(decodedAnchorAccountData).length} Items`
            :
              "No decoded data"}
          </div>
        </div>

      </div>
    </>
  );
}

export const hasAnchorIDL = async (address: PublicKey, url: string): Promise<Boolean> =>{
  const connection = new Connection(url);
      const provider = new Provider(connection, new Wallet(Keypair.generate()), {
        skipPreflight: false,
        commitment: 'confirmed',
        preflightCommitment: 'confirmed'
      })

      const program = await Program.at(address, provider)
      .catch(()=>{})
      return !!program
}

const renderAccountRow = (key: string, value: any) => {
  console.log('Value', value)
  let displayValue = value.toString()
  if (value.constructor.name === "PublicKey"){
    displayValue = <Address pubkey={value} link />
  } else if (displayValue === {}.toString()){
    if (Object.keys(value).length === 1){
      displayValue =  Object.keys(value)[0]
    } else {
      displayValue = JSON.stringify(value)
    }
    //<Address pubkey={account.pubkey} alignRight raw />
  }
  return (
    <tr key={key}>
      <td className="w-1 text-monospace">
        {camelToUnderscore(key)}
      </td>
      <td className="text-monospace">{displayValue}</td>
    </tr>
  );
};



function capitalizeFirstLetter(input: string) {
  return input.charAt(0).toUpperCase() + input.slice(1);
}

function equal (buf1:Buffer, buf2:Buffer)
{
  if (buf1.byteLength !== buf2.byteLength) return false;
  var dv1 = new Int8Array(buf1);
  var dv2 = new Int8Array(buf2);
  for (var i = 0 ; i !== buf1.byteLength ; i++)
  {
    if (dv1[i] !== dv2[i]) return false;
  }
  return true;
}

function camelToUnderscore(key:string) {
  var result = key.replace( /([A-Z])/g, " $1" );
  return result.split(' ').join('_').toLowerCase();
}