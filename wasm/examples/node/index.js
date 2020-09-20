const wasm = require('./../..');
const TextEncoder = require('util').TextEncoder;

(async () => {
    const instance = await wasm.waitReady();
    const keyPair = instance.GenerateKeyPair();
    const signature = instance.SignED25519(keyPair.public, keyPair.secret, new TextEncoder().encode('message to encode'));
    console.log(signature);
})();
