use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{error, fmt};
use std::collections::HashMap;
use solana_sdk::account::KeyedAccount;
use crate::spv_state::*;

// HeaderStore is a data structure that allows linked list style cheap appends and
// sequential reads, but also has a "lookup index" to speed up random access
enum HeaderStoreError {
    InvalidHeader,
    GroupExists,
    GroupDNE,
    InvalidBlockHeight,
}

// AccountList is a linked list of groups of blockheaders. It stores sequential blockheaders
#[derive(PartialEq, Eq, Hash)]
struct HeaderStore {
    pub index: Vec<Pubkey>,
    // number of header entries to include per group account
    pub groupSize: u16,
    // BaseHeight is the height of the first header in the first headerAccount
    pub baseHeight: u32,
    // topHeight is the running last header loaded
    pub topHeight:  u32,
    // account that administrates the headerstore and benefits from fees accrued
    pub owner: Pubkey,
}

impl HeaderStore {
    pub fn getGroup(self, blockHeight: u32) -> Result<Pubkey, HeaderStoreError> {
        if blockHeight < self.baseHeight || blockHeight > self.ceiling {
            Err(HeaderStoreError::InvalidBlockHeight)
        }
        else {
            let gheight: u32    = (blockheight - self.baseHeight) / self.groupSize as u32;
            let grouppk: Pubkey = self.index[gheight];
            Ok(grouppk)
        }
    }

    pub fn topGroup(self) -> Result<Pubkey, HeaderStoreError> {
        if self.index.len() == 0 {
            Err(HeaderStoreError::GroupDNE)
        }
        else {
            let grouppk: Pubkey = self.index.last().unwrap();
            Ok(grouppk)
        }
    }

    pub fn AppendHeader(mut self, blockheader: &BlockHeader) -> Result<(), SpvError> {
        match self.topGroup() {
            Some(n) => {
                let group = n;
            }
            None    => { // HeaderStore is empty need to create first group

            }
        }
        // access account data for group
    }

    pub fn ReplaceHeader(mut self, blockheader: &BlockHeader, blockheight: u32) -> Result<(), SpvError> {
        match self.getGroup(blockheight) {
            Some(n) => {
                let group = n;
            }
            None    => SpvError::InvalidHeader
        }
        // access account data for group
    }

    pub fn AppendGroup(mut self, pubkey: Pubkey) -> Result<(), SpvError> {
        if self.index.contains(&pubkey){
            // group to be appended is already in the index
            HeaderStoreError::GroupExists
        }

    }



}

struct HeaderAccount {
    // parent stores the pubkey of the parent AccountList
    pub parent: Pubkey,
    // stores a vec of BlockHeader structs
    pub headers: Vec<BlockHeader>,
    // next DataAccount in the chain or none if last
    pub next: Option<Pubkey>,

}
