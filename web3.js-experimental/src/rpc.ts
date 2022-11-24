import doThingBrowser from './doThing.browser';
import doThingNative from './doThing.native';
import doThingNode from './doThing';

import { sha256 } from '@noble/hashes/sha256';

export default function rpc() {
    if (__DEV__) {
        console.debug('We are in development mode.');
    }
    if (__BROWSER__) {
        console.log(doThingBrowser());
    }
    if (__NODEJS__) {
        console.log(doThingNode());
    }
    if (__REACTNATIVE__) {
        console.log(doThingNative());
    }
    console.log(sha256(new Uint8Array([0, 1, 2])));
}
