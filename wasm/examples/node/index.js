const wasm = require('./../..');
const TextEncoder = require('util').TextEncoder;

(async () => {
    const instance = await wasm.waitReady();
    const keyPair = instance.generateKeyPair();
    const signature = instance.signED25519(keyPair.publicKey, keyPair.secretKey, new TextEncoder().encode('message to encode'));
    console.log('Result', {
        keyPair, 
        signature
    })
})();
