use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{error, fmt};
use std::collections::HashMap;
use solana_sdk::account::KeyedAccount;
use crate::spv_state::*;

// HeaderStore is a data structure that allows linked list style cheap appends and
// sequential reads, but also has a "lookup index" to speed up random access
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum HeaderStoreError {
    InvalidHeader,
    GroupExists,
    GroupDNE,
    InvalidBlockHeight,
}

// AccountList is a linked list of groups of blockheaders. It stores sequential blockheaders
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeaderStore {
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
        if blockHeight < self.baseHeight || blockHeight > self.topHeight {
            Err(HeaderStoreError::InvalidBlockHeight)
        }
        else {
            let gheight = (blockHeight - self.baseHeight) / self.groupSize as u32;
            let grouppk: Pubkey = self.index[gheight as usize];
            Ok(grouppk)
        }
    }

    pub fn topGroup(self) -> Result<Pubkey, HeaderStoreError> {
        if self.index.len() == 0 {
            Err(HeaderStoreError::GroupDNE)
        }
        else {
            let grouppk: Pubkey = *self.index.last().unwrap();
            Ok(grouppk)
        }
    }

    pub fn AppendHeader(mut self, blockheader: &BlockHeader) -> Result<(), HeaderStoreError> {
        match self.topGroup() {
            Ok(n) => {
                let group = n;
                Ok(())
            }
            Err(E) => { // HeaderStore is empty need to create first group
                if HeaderStoreError::GroupDNE == E {
                    Ok(())
                }
                else {
                    Err(E)
                }
                //todo
            }
        }
    }

    pub fn ReplaceHeader(mut self, blockheader: &BlockHeader, blockheight: u32) -> Result<(), SpvError> {
        match self.getGroup(blockheight) {
            None    => {
                Err(SpvError::InvalidHeader)
            }
            Some(n) => {
                let group = n;
                Ok(())
            }
        }
        //todo
    }

    pub fn AppendGroup(mut self, pubkey: Pubkey) -> Result<(), HeaderStoreError> {
        if self.index.contains(&pubkey){
            // group to be appended is already in the index
            Err(HeaderStoreError::GroupExists)
        }
        else {
            Ok(())
            //insert actual function HeaderStoreError
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeaderAccountInfo {
    // parent stores the pubkey of the parent AccountList
    pub parent: Pubkey,
    // stores a vec of BlockHeader structs
    pub headers: Vec<BlockHeader>,
    // next DataAccount in the chain or none if last
    pub next: Option<Pubkey>,
}
