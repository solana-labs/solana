import * as web3 from '@solana/web3.js';

(async () => {
  // Connect to cluster
  var connection = new web3.Connection(
    web3.clusterApiUrl('devnet'),
    'confirmed',
  );

  // Generate a new wallet keypair and airdrop SOL
  var wallet1 = web3.Keypair.generate();
  var airdropSignature = await connection.requestAirdrop(
    wallet1.publicKey,
    web3.LAMPORTS_PER_SOL,
  );

  //wait for airdrop confirmation
  await connection.confirmTransaction(airdropSignature);

  // Generate a new wallet keypair and airdrop SOL
  var wallet2 = web3.Keypair.generate();
  var airdropSignature2 = await connection.requestAirdrop(
    wallet2.publicKey,
    web3.LAMPORTS_PER_SOL,
  );

  //wait for airdrop confirmation
  await connection.confirmTransaction(airdropSignature);

  // get both accounts' info through a single JSON RPC batch transaction
  // account data is bytecode that needs to be deserialized
  // serialization and deserialization is program specific
  let [account1, account2] = await connection.performBatchRequest([
    () => connection.getAccountInfo(wallet1.publicKey),
    () => connection.getAccountInfo(wallet2.publicKey)
  ]);
  console.log(account1, account2);
})();
