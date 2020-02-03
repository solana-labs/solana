use crate::remote_wallet::{
    initialize_wallet_manager, DerivationPath, RemoteWallet, RemoteWalletError, RemoteWalletInfo,
};
use dialoguer::{theme::ColorfulTheme, Select};
use log::*;
use semver::Version as FirmwareVersion;
use solana_sdk::{pubkey::Pubkey, signature::Signature, transaction::Transaction};
use std::{cmp::min, fmt, sync::Arc};

const APDU_TAG: u8 = 0x05;
const APDU_CLA: u8 = 0xe0;
const APDU_PAYLOAD_HEADER_LEN: usize = 7;

const SOL_DERIVATION_PATH_BE: [u8; 8] = [0x80, 0, 0, 44, 0x80, 0, 0x01, 0xF5]; // 44'/501', Solana

// const SOL_DERIVATION_PATH_BE: [u8; 8] = [0x80, 0, 0, 44, 0x80, 0, 0x00, 0x94]; // 44'/148', Stellar

/// Ledger vendor ID
const LEDGER_VID: u16 = 0x2c97;
/// Ledger product IDs: [Nano S and Nano X]
// TODO: do we need to support Blue?
const _LEDGER_PIDS: [u16; 2] = [0x0001, 0x0004]; // TODO: Nano S pid doesn't match expected LEDGER_PIDS value...
const LEDGER_TRANSPORT_HEADER_LEN: usize = 5;

const MAX_CHUNK_SIZE: usize = 255;

const HID_PACKET_SIZE: usize = 64 + HID_PREFIX_ZERO;

#[cfg(windows)]
const HID_PREFIX_ZERO: usize = 1;
#[cfg(not(windows))]
const HID_PREFIX_ZERO: usize = 0;

mod commands {
    pub const GET_APP_CONFIGURATION: u8 = 0x06;
    pub const GET_SOL_PUBKEY: u8 = 0x02;
    pub const SIGN_SOL_TRANSACTION: u8 = 0x04;
}

/// Ledger Wallet device
pub struct LedgerWallet {
    pub device: hidapi::HidDevice,
}

impl fmt::Debug for LedgerWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HidDevice")
    }
}

impl LedgerWallet {
    pub fn new(device: hidapi::HidDevice) -> Self {
        Self { device }
    }

    // Transport Protocol:
    //		* Communication Channel Id		(2 bytes big endian )
    //		* Command Tag				(1 byte)
    //		* Packet Sequence ID			(2 bytes big endian)
    //		* Payload				(Optional)
    //
    // Payload
    //		* APDU Total Length			(2 bytes big endian)
    //		* APDU_CLA				(1 byte)
    //		* APDU_INS				(1 byte)
    //		* APDU_P1				(1 byte)
    //		* APDU_P2				(1 byte)
    //		* APDU_LENGTH				(1 byte)
    //		* APDU_Payload				(Variable)
    //
    fn write(&self, command: u8, p1: u8, p2: u8, data: &[u8]) -> Result<(), RemoteWalletError> {
        let data_len = data.len();
        let mut offset = 0;
        let mut sequence_number = 0;
        let mut hid_chunk = [0_u8; HID_PACKET_SIZE];

        while sequence_number == 0 || offset < data_len {
            let header = if sequence_number == 0 {
                LEDGER_TRANSPORT_HEADER_LEN + APDU_PAYLOAD_HEADER_LEN
            } else {
                LEDGER_TRANSPORT_HEADER_LEN
            };
            let size = min(64 - header, data_len - offset);
            {
                let chunk = &mut hid_chunk[HID_PREFIX_ZERO..];
                chunk[0..5].copy_from_slice(&[
                    0x01,
                    0x01,
                    APDU_TAG,
                    (sequence_number >> 8) as u8,
                    (sequence_number & 0xff) as u8,
                ]);

                if sequence_number == 0 {
                    let data_len = data.len() + 5;
                    chunk[5..12].copy_from_slice(&[
                        (data_len >> 8) as u8,
                        (data_len & 0xff) as u8,
                        APDU_CLA,
                        command,
                        p1,
                        p2,
                        data.len() as u8,
                    ]);
                }

                chunk[header..header + size].copy_from_slice(&data[offset..offset + size]);
            }
            trace!("Ledger write {:?}", &hid_chunk[..]);
            let n = self.device.write(&hid_chunk[..])?;
            if n < size + header {
                return Err(RemoteWalletError::Protocol("Write data size mismatch"));
            }
            offset += size;
            sequence_number += 1;
            if sequence_number >= 0xffff {
                return Err(RemoteWalletError::Protocol(
                    "Maximum sequence number reached",
                ));
            }
        }
        Ok(())
    }

    // Transport Protocol:
    //		* Communication Channel Id		(2 bytes big endian )
    //		* Command Tag				(1 byte)
    //		* Packet Sequence ID			(2 bytes big endian)
    //		* Payload				(Optional)
    //
    // Payload
    //		* APDU Total Length			(2 bytes big endian)
    //		* APDU_CLA				(1 byte)
    //		* APDU_INS				(1 byte)
    //		* APDU_P1				(1 byte)
    //		* APDU_P2				(1 byte)
    //		* APDU_LENGTH				(1 byte)
    //		* APDU_Payload				(Variable)
    //
    fn read(&self) -> Result<Vec<u8>, RemoteWalletError> {
        let mut message_size = 0;
        let mut message = Vec::new();

        // terminate the loop if `sequence_number` reaches its max_value and report error
        for chunk_index in 0..=0xffff {
            let mut chunk: [u8; HID_PACKET_SIZE] = [0; HID_PACKET_SIZE];
            let chunk_size = self.device.read(&mut chunk)?;
            trace!("Ledger read {:?}", &chunk[..]);
            if chunk_size < LEDGER_TRANSPORT_HEADER_LEN
                || chunk[0] != 0x01
                || chunk[1] != 0x01
                || chunk[2] != APDU_TAG
            {
                return Err(RemoteWalletError::Protocol("Unexpected chunk header"));
            }
            let seq = (chunk[3] as usize) << 8 | (chunk[4] as usize);
            if seq != chunk_index {
                return Err(RemoteWalletError::Protocol("Unexpected chunk header"));
            }

            let mut offset = 5;
            if seq == 0 {
                // Read message size and status word.
                if chunk_size < 7 {
                    return Err(RemoteWalletError::Protocol("Unexpected chunk header"));
                }
                message_size = (chunk[5] as usize) << 8 | (chunk[6] as usize);
                offset += 2;
            }
            message.extend_from_slice(&chunk[offset..chunk_size]);
            message.truncate(message_size);
            if message.len() == message_size {
                break;
            }
        }
        if message.len() < 2 {
            return Err(RemoteWalletError::Protocol("No status word"));
        }
        let status =
            (message[message.len() - 2] as usize) << 8 | (message[message.len() - 1] as usize);
        trace!("Read status {:x}", status);
        #[allow(clippy::match_overlapping_arm)]
        match status {
            // TODO: These need to be aligned with solana Ledger app error codes, and clippy allowance removed
            0x6700 => Err(RemoteWalletError::Protocol("Incorrect length")),
            0x6982 => Err(RemoteWalletError::Protocol(
                "Security status not satisfied (Canceled by user)",
            )),
            0x6a80 => Err(RemoteWalletError::Protocol("Invalid data")),
            0x6a82 => Err(RemoteWalletError::Protocol("File not found")),
            0x6a85 => Err(RemoteWalletError::UserCancel),
            0x6b00 => Err(RemoteWalletError::Protocol("Incorrect parameters")),
            0x6d00 => Err(RemoteWalletError::Protocol(
                "Not implemented. Make sure the Ledger Solana Wallet app is running.",
            )),
            0x6faa => Err(RemoteWalletError::Protocol(
                "Your Ledger needs to be unplugged",
            )),
            0x6f00..=0x6fff => Err(RemoteWalletError::Protocol("Internal error")),
            0x9000 => Ok(()),
            _ => Err(RemoteWalletError::Protocol("Unknown error")),
        }?;
        let new_len = message.len() - 2;
        message.truncate(new_len);
        Ok(message)
    }

    fn send_apdu(
        &self,
        command: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
    ) -> Result<Vec<u8>, RemoteWalletError> {
        self.write(command, p1, p2, data)?;
        self.read()
    }

    fn get_firmware_version(&self) -> Result<FirmwareVersion, RemoteWalletError> {
        let ver = self.send_apdu(commands::GET_APP_CONFIGURATION, 0, 0, &[])?;
        if ver.len() != 4 {
            return Err(RemoteWalletError::Protocol("Version packet size mismatch"));
        }
        Ok(FirmwareVersion::new(
            ver[1].into(),
            ver[2].into(),
            ver[3].into(),
        ))
    }

    fn get_derivation_path(&self, derivation: DerivationPath) -> Vec<u8> {
        let byte = if derivation.change.is_some() { 4 } else { 3 };
        let mut concat_derivation = vec![byte];
        concat_derivation.extend_from_slice(&SOL_DERIVATION_PATH_BE);
        concat_derivation.extend_from_slice(&[0x80, 0]);
        concat_derivation.extend_from_slice(&derivation.account.to_be_bytes());
        if let Some(change) = derivation.change {
            concat_derivation.extend_from_slice(&[0x80, 0]);
            concat_derivation.extend_from_slice(&change.to_be_bytes());
        }
        concat_derivation
    }
}

impl RemoteWallet for LedgerWallet {
    fn read_device(
        &self,
        dev_info: &hidapi::HidDeviceInfo,
    ) -> Result<RemoteWalletInfo, RemoteWalletError> {
        let manufacturer = dev_info
            .manufacturer_string
            .clone()
            .unwrap_or_else(|| "Unknown".to_owned())
            .to_lowercase()
            .replace(" ", "-");
        let model = dev_info
            .product_string
            .clone()
            .unwrap_or_else(|| "Unknown".to_owned())
            .to_lowercase()
            .replace(" ", "-");
        let serial = dev_info
            .serial_number
            .clone()
            .unwrap_or_else(|| "Unknown".to_owned());
        self.get_pubkey(DerivationPath::default())
            .map(|pubkey| RemoteWalletInfo {
                model,
                manufacturer,
                serial,
                pubkey,
            })
    }

    fn get_pubkey(&self, derivation: DerivationPath) -> Result<Pubkey, RemoteWalletError> {
        // let ledger_version = self.get_firmware_version()?;
        // if ledger_version < FirmwareVersion::new(1, 0, 3) {
        //     return Err(RemoteWalletError::Protocol(
        //         "Ledger version 1.0.3 is required",
        //     ));
        // }

        let derivation_path = self.get_derivation_path(derivation);

        let key = self.send_apdu(commands::GET_SOL_PUBKEY, 0, 0, &derivation_path)?;
        if key.len() != 32 {
            return Err(RemoteWalletError::Protocol("Key packet size mismatch"));
        }
        Ok(Pubkey::new(&key))
    }

    fn sign_transaction(
        &self,
        derivation: DerivationPath,
        transaction: Transaction,
    ) -> Result<Signature, RemoteWalletError> {
        // TODO: see if need version check here
        let _version = self.get_firmware_version()?;
        // if version < FirmwareVersion::new(1, 0, 8) {
        //     return Err(RemoteWalletError::Protocol(
        //         "Signing personal messages with Ledger requires version 1.0.8",
        //     ));
        // }

        let mut chunk = [0_u8; MAX_CHUNK_SIZE];
        let derivation_path = self.get_derivation_path(derivation);
        let data = transaction.message_data();

        // Copy the address of the key (only done once)
        chunk[0..derivation_path.len()].copy_from_slice(&derivation_path);

        let key_length = derivation_path.len();
        let max_payload_size = MAX_CHUNK_SIZE - key_length;
        let data_len = data.len();

        let mut result = Vec::new();
        let mut offset = 0;

        while offset < data_len {
            let p1 = if offset == 0 { 0 } else { 0x80 };
            let take = min(max_payload_size, data_len - offset);

            // Fetch piece of data and copy it!
            {
                let (_key, d) = &mut chunk.split_at_mut(key_length);
                let (dst, _rem) = &mut d.split_at_mut(take);
                dst.copy_from_slice(&data[offset..(offset + take)]);
            }

            result = self.send_apdu(
                commands::SIGN_SOL_TRANSACTION,
                p1,
                0,
                &chunk[0..(key_length + take)],
            )?;
            offset += take;
        }

        if result.len() != 64 {
            return Err(RemoteWalletError::Protocol(
                "Signature packet size mismatch",
            ));
        }
        Ok(Signature::new(&result))
    }
}

/// Check if the detected device is a valid `Ledger device` by checking both the product ID and the vendor ID
pub fn is_valid_ledger(vendor_id: u16, _product_id: u16) -> bool {
    // vendor_id == LEDGER_VID && LEDGER_PIDS.contains(&product_id)
    vendor_id == LEDGER_VID
}

///
pub fn get_ledger_from_info(
    info: RemoteWalletInfo,
) -> Result<Arc<LedgerWallet>, RemoteWalletError> {
    let wallet_manager = initialize_wallet_manager();
    let _device_count = wallet_manager.update_devices()?;
    let devices = wallet_manager.list_devices();
    let (pubkeys, device_paths): (Vec<Pubkey>, Vec<String>) = devices
        .iter()
        .filter(|&device_info| device_info == &info)
        .map(|device_info| (device_info.pubkey, device_info.get_pretty_path()))
        .unzip();
    if pubkeys.is_empty() {
        return Err(RemoteWalletError::NoDeviceFound);
    }
    let wallet_base_pubkey = if pubkeys.len() > 1 {
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Multiple hardware wallets found. Please select a device")
            .default(0)
            .items(&device_paths[..])
            .interact()
            .unwrap();
        pubkeys[selection]
    } else {
        pubkeys[0]
    };
    wallet_manager.get_ledger(&wallet_base_pubkey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_wallet::initialize_wallet_manager;
    use std::collections::HashSet;

    /// This test can't be run without an actual ledger device connected with the `Ledger Wallet Solana application` running
    #[test]
    fn ledger_pubkey_test() {
        let wallet_manager = initialize_wallet_manager();

        // Update device list
        wallet_manager.update_devices().expect("No Ledger found, make sure you have a unlocked Ledger connected with the Ledger Wallet Solana running");
        assert!(wallet_manager.list_devices().len() > 0);

        // Fetch the base pubkey of a connected ledger device
        let ledger_base_pubkey = wallet_manager
            .list_devices()
            .iter()
            .filter(|d| d.manufacturer == "ledger".to_string())
            .nth(0)
            .map(|d| d.pubkey.clone())
            .expect("No ledger device detected");

        let ledger = wallet_manager
            .get_ledger(&ledger_base_pubkey)
            .expect("get device");

        let mut pubkey_set = HashSet::new();
        pubkey_set.insert(ledger_base_pubkey);

        let pubkey_0_0 = ledger
            .get_pubkey(DerivationPath {
                account: 0,
                change: Some(0),
            })
            .expect("get pubkey");
        pubkey_set.insert(pubkey_0_0);
        let pubkey_0_1 = ledger
            .get_pubkey(DerivationPath {
                account: 0,
                change: Some(1),
            })
            .expect("get pubkey");
        pubkey_set.insert(pubkey_0_1);
        let pubkey_1 = ledger
            .get_pubkey(DerivationPath {
                account: 1,
                change: None,
            })
            .expect("get pubkey");
        pubkey_set.insert(pubkey_1);
        let pubkey_1_0 = ledger
            .get_pubkey(DerivationPath {
                account: 1,
                change: Some(0),
            })
            .expect("get pubkey");
        pubkey_set.insert(pubkey_1_0);

        assert_eq!(pubkey_set.len(), 5); // Ensure keys at various derivation paths are unique
    }
}
