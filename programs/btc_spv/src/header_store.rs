#[allow(unused_imports)]
use crate::spv_state::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

// HeaderStore is a data structure that allows linked list style cheap appends and
// sequential reads, but also has a "lookup index" to speed up random access
#[derive(Debug, PartialEq, Eq, Clone)]
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
    pub group_size: u16,
    // base_height is the height of the first header in the first headerAccount
    pub base_height: u32,
    // top_height is the running last header loaded
    pub top_height: u32,
    // account that administrates the headerstore and benefits from fees accrued
    pub owner: Pubkey,
}

impl HeaderStore {
    pub fn get_group(self, block_height: u32) -> Result<Pubkey, HeaderStoreError> {
        if block_height < self.base_height || block_height > self.top_height {
            Err(HeaderStoreError::InvalidBlockHeight)
        } else {
            let gheight = (block_height - self.base_height) / u32::from(self.group_size);
            let grouppk: Pubkey = self.index[gheight as usize];
            Ok(grouppk)
        }
    }

    pub fn top_group(self) -> Result<Pubkey, HeaderStoreError> {
        if self.index.is_empty() {
            Err(HeaderStoreError::GroupDNE)
        } else {
            let grouppk: Pubkey = *self.index.last().unwrap();
            Ok(grouppk)
        }
    }

    pub fn append_header(mut self, blockheader: &BlockHeader) -> Result<(), HeaderStoreError> {
        match self.top_group() {
            Ok(n) => {
                let group = n;
                Ok(())
            }
            Err(e) => {
                // HeaderStore is empty need to create first group
                if HeaderStoreError::GroupDNE == e {
                    Ok(())
                } else {
                    Err(e)
                }
                //reinsert
            }
        }
    }

    pub fn replace_header(
        mut self,
        blockheader: &BlockHeader,
        block_height: u32,
    ) -> Result<(), HeaderStoreError> {
        match self.get_group(block_height) {
            Err(e) => Err(HeaderStoreError::InvalidHeader),
            Ok(n) => {
                let group = n;
                Ok(())
            }
        }
        //reinsert
    }

    pub fn append_group(mut self, pubkey: Pubkey) -> Result<(), HeaderStoreError> {
        if self.index.contains(&pubkey) {
            // group to be appended is already in the index
            Err(HeaderStoreError::GroupExists)
        } else {
            Ok(())
            //reinsert
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
