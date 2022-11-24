import doThingBrowser from './doThing.browser';
import doThingNative from './doThing.native';
import doThingNode from './doThing';

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
}
