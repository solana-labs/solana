import {sha512} from '@noble/hashes/sha512';
import * as ed25519 from '@noble/ed25519';

ed25519.utils.sha512Sync = (...m) => sha512(ed25519.utils.concatBytes(...m));

export const generatePrivateKey = ed25519.utils.randomPrivateKey;
export const getPublicKey = ed25519.sync.getPublicKey;
