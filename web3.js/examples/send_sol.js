import * as web3 from '@solana/web3.js';

(async () => {
  // Connect to cluster
  var connection = new web3.Connection(
    web3.clusterApiUrl('devnet'),
    'confirmed',
  );

  // Generate a new random public key
  var from = web3.Keypair.generate();
  var airdropSignature = await connection.requestAirdrop(
    from.publicKey,
    web3.LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(airdropSignature);

  // Generate a new random public key
  var to = web3.Keypair.generate();

  // Add transfer instruction to transaction
  var transaction = new web3.Transaction().add(
    web3.SystemProgram.transfer({
      fromPubkey: from.publicKey,
      toPubkey: to.publicKey,
      lamports: web3.LAMPORTS_PER_SOL / 100,
    }),
  );

  // Sign transaction, broadcast, and confirm
  var signature = await web3.sendAndConfirmTransaction(
    connection,
    transaction,
    [from],
  );
  console.log('SIGNATURE', signature);
})();
