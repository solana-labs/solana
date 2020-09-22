use crate::cli_output::{CliSignOnlyData, OutputFormat};
use serde_json::Value;
use solana_clap_utils::keypair::presigner_from_pubkey_sigs;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Presigner, Signature},
    transaction::Transaction,
};
use std::str::FromStr;

pub fn return_signers(
    tx: &Transaction,
    output_format: &OutputFormat,
) -> Result<String, Box<dyn std::error::Error>> {
    let verify_results = tx.verify_with_results();
    let mut signers = Vec::new();
    let mut absent = Vec::new();
    let mut bad_sig = Vec::new();
    tx.signatures
        .iter()
        .zip(tx.message.account_keys.iter())
        .zip(verify_results.into_iter())
        .for_each(|((sig, key), res)| {
            if res {
                signers.push(format!("{}={}", key, sig))
            } else if *sig == Signature::default() {
                absent.push(key.to_string());
            } else {
                bad_sig.push(key.to_string());
            }
        });

    let cli_command = CliSignOnlyData {
        blockhash: tx.message.recent_blockhash.to_string(),
        signers,
        absent,
        bad_sig,
    };

    Ok(output_format.formatted_string(&cli_command))
}

pub struct SignOnly {
    pub blockhash: Hash,
    pub present_signers: Vec<(Pubkey, Signature)>,
    pub absent_signers: Vec<Pubkey>,
    pub bad_signers: Vec<Pubkey>,
}

impl SignOnly {
    pub fn has_all_signers(&self) -> bool {
        self.absent_signers.is_empty() && self.bad_signers.is_empty()
    }

    pub fn presigner_of(&self, pubkey: &Pubkey) -> Option<Presigner> {
        presigner_from_pubkey_sigs(pubkey, &self.present_signers)
    }
}

pub fn parse_sign_only_reply_string(reply: &str) -> SignOnly {
    let object: Value = serde_json::from_str(&reply).unwrap();
    let blockhash_str = object.get("blockhash").unwrap().as_str().unwrap();
    let blockhash = blockhash_str.parse::<Hash>().unwrap();
    let mut present_signers: Vec<(Pubkey, Signature)> = Vec::new();
    let signer_strings = object.get("signers");
    if let Some(sig_strings) = signer_strings {
        present_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|signer_string| {
                let mut signer = signer_string.as_str().unwrap().split('=');
                let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
                let sig = Signature::from_str(signer.next().unwrap()).unwrap();
                (key, sig)
            })
            .collect();
    }
    let mut absent_signers: Vec<Pubkey> = Vec::new();
    let signer_strings = object.get("absent");
    if let Some(sig_strings) = signer_strings {
        absent_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|val| {
                let s = val.as_str().unwrap();
                Pubkey::from_str(s).unwrap()
            })
            .collect();
    }
    let mut bad_signers: Vec<Pubkey> = Vec::new();
    let signer_strings = object.get("badSig");
    if let Some(sig_strings) = signer_strings {
        bad_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|val| {
                let s = val.as_str().unwrap();
                Pubkey::from_str(s).unwrap()
            })
            .collect();
    }
    SignOnly {
        blockhash,
        present_signers,
        absent_signers,
        bad_signers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        message::Message,
        pubkey::Pubkey,
        signature::{keypair_from_seed, NullSigner, Signature, Signer, SignerError},
        system_instruction,
        transaction::Transaction,
    };

    #[test]
    fn test_return_signers() {
        struct BadSigner {
            pubkey: Pubkey,
        }

        impl BadSigner {
            pub fn new(pubkey: Pubkey) -> Self {
                Self { pubkey }
            }
        }

        impl Signer for BadSigner {
            fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
                Ok(self.pubkey)
            }

            fn try_sign_message(&self, _message: &[u8]) -> Result<Signature, SignerError> {
                Ok(Signature::new(&[1u8; 64]))
            }
        }

        let present: Box<dyn Signer> = Box::new(keypair_from_seed(&[2u8; 32]).unwrap());
        let absent: Box<dyn Signer> = Box::new(NullSigner::new(&Pubkey::new(&[3u8; 32])));
        let bad: Box<dyn Signer> = Box::new(BadSigner::new(Pubkey::new(&[4u8; 32])));
        let to = Pubkey::new(&[5u8; 32]);
        let nonce = Pubkey::new(&[6u8; 32]);
        let from = present.pubkey();
        let fee_payer = absent.pubkey();
        let nonce_auth = bad.pubkey();
        let mut tx = Transaction::new_unsigned(Message::new_with_nonce(
            vec![system_instruction::transfer(&from, &to, 42)],
            Some(&fee_payer),
            &nonce,
            &nonce_auth,
        ));

        let signers = vec![present.as_ref(), absent.as_ref(), bad.as_ref()];
        let blockhash = Hash::new(&[7u8; 32]);
        tx.try_partial_sign(&signers, blockhash).unwrap();
        let res = return_signers(&tx, &OutputFormat::JsonCompact).unwrap();
        let sign_only = parse_sign_only_reply_string(&res);
        assert_eq!(sign_only.blockhash, blockhash);
        assert_eq!(sign_only.present_signers[0].0, present.pubkey());
        assert_eq!(sign_only.absent_signers[0], absent.pubkey());
        assert_eq!(sign_only.bad_signers[0], bad.pubkey());
    }
}
