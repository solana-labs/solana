import { useState } from 'react';
import { Idl, Program, Provider } from "@project-serum/anchor";
import { Connection, Keypair } from '@solana/web3.js';
import { NodeWallet } from '@metaplex/js';

let cachedAnchorPromises: {
    [key: string]:
    { _type: 'promise', promise: Promise<Program<Idl> | null> } |
    // { _type: 'promise', promise: Promise<void> } |
    { _type: 'result', result: Program<Idl> | null }
} = {};

export function useAnchorProgram(
    programAddress: string,
    url: string,
): Program | null {
    const key = `${programAddress}-${url}`;
    const cacheEntry = cachedAnchorPromises[key];
    const [anchorProgram, setAnchorProgram] = useState<Program<Idl> | null>(
        () => cacheEntry?._type === 'result' ? cacheEntry.result : null

    );

    if (cacheEntry === undefined) {
        const promise = Program.at(programAddress, new Provider(new Connection(url), new NodeWallet(Keypair.generate()), {}))
            .then((program) => {
                cachedAnchorPromises[key] = { _type: 'result', result: program };
                setAnchorProgram(program);
                return program;
            })
            .catch((_) => {
                cachedAnchorPromises[key] = { _type: 'result', result: null };
                setAnchorProgram(null);
                return null;
            })
        cachedAnchorPromises[key] = {
            _type: 'promise',
            promise
        };
        throw promise;
    } else if (cacheEntry._type === 'promise') {
        cacheEntry.promise.then((result) => {
            setAnchorProgram(result);
        })
        throw cacheEntry.promise;
    }
    return anchorProgram;
}