const sdk = require('./../..');
const TextEncoder = require('util').TextEncoder;

(async () => {
    await sdk.waitReady();
    const keyPair = sdk.ed25519.keypair.generate();
    const signature = sdk.ed25519.sign(keyPair.publicKey, keyPair.secretKey, new TextEncoder().encode('message to encode'));
    console.log('KeyPair and Signature', {
        keyPair, 
        signature
    })

    console.log('KeyPair from secret key', sdk.ed25519.keypair.fromSecretKey(keyPair.secretKey));

    console.log('Sha256', sdk.hasher.sha256(signature));
})();
