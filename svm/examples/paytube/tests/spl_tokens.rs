mod setup;

use {
    setup::{
        get_token_account_balance, mint_account, system_account, token_account,
        TestValidatorContext,
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer},
    solana_svm_example_paytube::{transaction::PayTubeTransaction, PayTubeChannel},
    spl_associated_token_account::get_associated_token_address,
};

#[test]
fn test_spl_tokens() {
    let mint = Pubkey::new_unique();

    let alice = Keypair::new();
    let bob = Keypair::new();
    let will = Keypair::new();

    let alice_pubkey = alice.pubkey();
    let alice_token_account_pubkey = get_associated_token_address(&alice_pubkey, &mint);

    let bob_pubkey = bob.pubkey();
    let bob_token_account_pubkey = get_associated_token_address(&bob_pubkey, &mint);

    let will_pubkey = will.pubkey();
    let will_token_account_pubkey = get_associated_token_address(&will_pubkey, &mint);

    let accounts = vec![
        (mint, mint_account()),
        (alice_pubkey, system_account(10_000_000)),
        (
            alice_token_account_pubkey,
            token_account(&alice_pubkey, &mint, 10),
        ),
        (bob_pubkey, system_account(10_000_000)),
        (
            bob_token_account_pubkey,
            token_account(&bob_pubkey, &mint, 10),
        ),
        (will_pubkey, system_account(10_000_000)),
        (
            will_token_account_pubkey,
            token_account(&will_pubkey, &mint, 10),
        ),
    ];

    let context = TestValidatorContext::start_with_accounts(accounts);
    let test_validator = &context.test_validator;
    let payer = context.payer.insecure_clone();

    let rpc_client = test_validator.get_rpc_client();

    let paytube_channel = PayTubeChannel::new(vec![payer, alice, bob, will], rpc_client);

    paytube_channel.process_paytube_transfers(&[
        // Alice -> Bob 2
        PayTubeTransaction {
            from: alice_pubkey,
            to: bob_pubkey,
            amount: 2,
            mint: Some(mint),
        },
        // Bob -> Will 5
        PayTubeTransaction {
            from: bob_pubkey,
            to: will_pubkey,
            amount: 5,
            mint: Some(mint),
        },
        // Alice -> Bob 2
        PayTubeTransaction {
            from: alice_pubkey,
            to: bob_pubkey,
            amount: 2,
            mint: Some(mint),
        },
        // Will -> Alice 1
        PayTubeTransaction {
            from: will_pubkey,
            to: alice_pubkey,
            amount: 1,
            mint: Some(mint),
        },
    ]);

    // Ledger:
    // Alice:   10 - 2 - 2 + 1  = 7
    // Bob:     10 + 2 - 5 + 2  = 9
    // Will:    10 + 5 - 1      = 14
    let rpc_client = test_validator.get_rpc_client();
    assert_eq!(
        get_token_account_balance(rpc_client.get_account(&alice_token_account_pubkey).unwrap()),
        7
    );
    assert_eq!(
        get_token_account_balance(rpc_client.get_account(&bob_token_account_pubkey).unwrap()),
        9
    );
    assert_eq!(
        get_token_account_balance(rpc_client.get_account(&will_token_account_pubkey).unwrap()),
        14
    );
}
