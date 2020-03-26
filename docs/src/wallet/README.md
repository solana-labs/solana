# Solana Wallet
This document describes the different wallet options that are available to users
of Solana who want to be able to send, receive and interact with
SOL tokens on the Solana blockchain.

## What is a Wallet?
A crypto *wallet* is a scheme that allows users to send, receive, 
and track ownership of cryptocurrencies.  Wallets can take many forms. 
A wallet might be a directory or file in your computer's file system, 
a piece of paper, or a specialized device called a *hardware wallet*.
There are also various smartphone apps and computer programs 
that can create and manage user-friendly wallets.

A wallet is primarily composed of a *private key* and its 
cryptographically-derived *public key*.  A private key and its corresponding 
public key are together known as a *keypair*.  
A wallet is simply a way for a user to keep track of and manage 
one or more keypairs.

The *public key* (commonly shortened to *pubkey*) is known as the wallet's 
*receiving address* or simply its *address*.  The wallet address can be shared 
and displayed freely.  When another party is going to send some amount of 
cryptocurrency to a wallet, they need to know the wallet's receiving address.  
Depending on a blockchain's implementation, the address can also be used to view
certain information about a wallet, such as viewing the balance, 
but has no ability to change anything about the wallet or withdraw any tokens.

The *private key* is required to digitally sign any transactions to send 
cryptocurrencies to another address or to make any changes to the wallet. 
The private key **must never be shared**.  If someone gains access to the 
private key to a wallet, they can withdraw all the tokens it contains.  
If the private key for a wallet is lost, any tokens that have been sent 
to that wallet's address are **permanently lost**.

Different wallet solutions are available to manage the compromise between 
securely storing and backing up the private key and providing a convenient way
to interact with the keypair and sign transactions to use/spend the tokens.

Some wallets are easier to use than others.  Some are more secure than others.  
[Multiple types of wallets support Solana](supported-wallets.md) from which
users can choose.

If you want to be able to receive SOL tokens on the Solana blockchain, 
you first will need to create a wallet.