use crate::{
    ledger_error::LedgerError,
    remote_wallet::{
        DerivationPath, RemoteWallet, RemoteWalletError, RemoteWalletInfo, RemoteWalletManager,
    },
};
use console::Emoji;
use dialoguer::{theme::ColorfulTheme, Select};
use log::*;
use num_traits::FromPrimitive;
use semver::Version as FirmwareVersion;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::{cmp::min, fmt, sync::Arc};

static CHECK_MARK: Emoji = Emoji("✅ ", "");

const DEPRECATE_VERSION_BEFORE: FirmwareVersion = FirmwareVersion {
    major: 0,
    minor: 2,
    patch: 0,
    pre: Vec::new(),
    build: Vec::new(),
};

const HARDENED_BIT: u32 = 1 << 31;

const APDU_TAG: u8 = 0x05;
const APDU_CLA: u8 = 0xe0;
const APDU_PAYLOAD_HEADER_LEN: usize = 7;
const DEPRECATED_APDU_PAYLOAD_HEADER_LEN: usize = 8;
const P1_NON_CONFIRM: u8 = 0x00;
const P1_CONFIRM: u8 = 0x01;
const P2_EXTEND: u8 = 0x01;
const P2_MORE: u8 = 0x02;
const MAX_CHUNK_SIZE: usize = 255;

const APDU_SUCCESS_CODE: usize = 0x9000;

const SOL_DERIVATION_PATH_BE: [u8; 8] = [0x80, 0, 0, 44, 0x80, 0, 0x01, 0xF5]; // 44'/501', Solana

/// Ledger vendor ID
const LEDGER_VID: u16 = 0x2c97;
/// Ledger product IDs: Nano S and Nano X
const LEDGER_NANO_S_PIDS: [u16; 33] = [
    0x0001, 0x1000, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009, 0x100a,
    0x100b, 0x100c, 0x100d, 0x100e, 0x100f, 0x1010, 0x1011, 0x1012, 0x1013, 0x1014, 0x1015, 0x1016,
    0x1017, 0x1018, 0x1019, 0x101a, 0x101b, 0x101c, 0x101d, 0x101e, 0x101f,
];
const LEDGER_NANO_X_PIDS: [u16; 33] = [
    0x0004, 0x4000, 0x4001, 0x4002, 0x4003, 0x4004, 0x4005, 0x4006, 0x4007, 0x4008, 0x4009, 0x400a,
    0x400b, 0x400c, 0x400d, 0x400e, 0x400f, 0x4010, 0x4011, 0x4012, 0x4013, 0x4014, 0x4015, 0x4016,
    0x4017, 0x4018, 0x4019, 0x401a, 0x401b, 0x401c, 0x401d, 0x401e, 0x401f,
];
const LEDGER_TRANSPORT_HEADER_LEN: usize = 5;

const HID_PACKET_SIZE: usize = 64 + HID_PREFIX_ZERO;

#[cfg(windows)]
const HID_PREFIX_ZERO: usize = 1;
#[cfg(not(windows))]
const HID_PREFIX_ZERO: usize = 0;

mod commands {
    pub const DEPRECATED_GET_APP_CONFIGURATION: u8 = 0x01;
    pub const DEPRECATED_GET_PUBKEY: u8 = 0x02;
    pub const DEPRECATED_SIGN_MESSAGE: u8 = 0x03;
    pub const GET_APP_CONFIGURATION: u8 = 0x04;
    pub const GET_PUBKEY: u8 = 0x05;
    pub const SIGN_MESSAGE: u8 = 0x06;
}

/// Ledger Wallet device
pub struct LedgerWallet {
    pub device: hidapi::HidDevice,
    pub pretty_path: String,
    pub version: FirmwareVersion,
}

impl fmt::Debug for LedgerWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HidDevice")
    }
}

impl LedgerWallet {
    pub fn new(device: hidapi::HidDevice) -> Self {
        Self {
            device,
            pretty_path: String::default(),
            version: FirmwareVersion::new(0, 0, 0),
        }
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
    //		* APDU_LENGTH 	        (1 byte (2 bytes DEPRECATED))
    //		* APDU_Payload				(Variable)
    //
    fn write(
        &self,
        command: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
        outdated_app: bool,
    ) -> Result<(), RemoteWalletError> {
        let data_len = data.len();
        let mut offset = 0;
        let mut sequence_number = 0;
        let mut hid_chunk = [0_u8; HID_PACKET_SIZE];

        while sequence_number == 0 || offset < data_len {
            let header = if sequence_number == 0 {
                if outdated_app {
                    LEDGER_TRANSPORT_HEADER_LEN + DEPRECATED_APDU_PAYLOAD_HEADER_LEN
                } else {
                    LEDGER_TRANSPORT_HEADER_LEN + APDU_PAYLOAD_HEADER_LEN
                }
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
                    if outdated_app {
                        let data_len = data.len() + 6;
                        chunk[5..13].copy_from_slice(&[
                            (data_len >> 8) as u8,
                            (data_len & 0xff) as u8,
                            APDU_CLA,
                            command,
                            p1,
                            p2,
                            (data.len() >> 8) as u8,
                            data.len() as u8,
                        ]);
                    } else {
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
        Self::parse_status(status)?;
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
        self.write(command, p1, p2, data, self.outdated_app())?;
        if p1 == P1_CONFIRM && is_last_part(p2) {
            println!(
                "Waiting for your approval on {} {}",
                self.name(),
                self.pretty_path
            );
            let result = self.read()?;
            println!("{}Approved", CHECK_MARK);
            Ok(result)
        } else {
            self.read()
        }
    }

    fn get_firmware_version(&self) -> Result<FirmwareVersion, RemoteWalletError> {
        if let Ok(version) = self.send_apdu(commands::GET_APP_CONFIGURATION, 0, 0, &[]) {
            if version.len() != 5 {
                return Err(RemoteWalletError::Protocol("Version packet size mismatch"));
            }
            Ok(FirmwareVersion::new(
                version[2].into(),
                version[3].into(),
                version[4].into(),
            ))
        } else {
            let version = self.send_apdu(commands::DEPRECATED_GET_APP_CONFIGURATION, 0, 0, &[])?;
            if version.len() != 4 {
                return Err(RemoteWalletError::Protocol("Version packet size mismatch"));
            }
            Ok(FirmwareVersion::new(
                version[1].into(),
                version[2].into(),
                version[3].into(),
            ))
        }
    }

    fn outdated_app(&self) -> bool {
        self.version < DEPRECATE_VERSION_BEFORE
    }

    fn parse_status(status: usize) -> Result<(), RemoteWalletError> {
        if status == APDU_SUCCESS_CODE {
            Ok(())
        } else if let Some(err) = LedgerError::from_usize(status) {
            Err(err.into())
        } else {
            Err(RemoteWalletError::Protocol("Unknown error"))
        }
    }
}

impl RemoteWallet for LedgerWallet {
    fn name(&self) -> &str {
        "Ledger hardware wallet"
    }

    fn read_device(
        &mut self,
        dev_info: &hidapi::DeviceInfo,
    ) -> Result<RemoteWalletInfo, RemoteWalletError> {
        let manufacturer = dev_info
            .manufacturer_string()
            .clone()
            .unwrap_or("Unknown")
            .to_lowercase()
            .replace(" ", "-");
        let model = dev_info
            .product_string()
            .clone()
            .unwrap_or("Unknown")
            .to_lowercase()
            .replace(" ", "-");
        let serial = dev_info
            .serial_number()
            .clone()
            .unwrap_or("Unknown")
            .to_string();
        let version = self.get_firmware_version()?;
        self.version = version;
        let pubkey_result = self.get_pubkey(&DerivationPath::default(), false);
        let (pubkey, error) = match pubkey_result {
            Ok(pubkey) => (pubkey, None),
            Err(err) => (Pubkey::default(), Some(err)),
        };
        Ok(RemoteWalletInfo {
            model,
            manufacturer,
            serial,
            pubkey,
            error,
        })
    }

    fn get_pubkey(
        &self,
        derivation_path: &DerivationPath,
        confirm_key: bool,
    ) -> Result<Pubkey, RemoteWalletError> {
        let derivation_path = extend_and_serialize(derivation_path);

        let key = self.send_apdu(
            if self.outdated_app() {
                commands::DEPRECATED_GET_PUBKEY
            } else {
                commands::GET_PUBKEY
            },
            if confirm_key {
                P1_CONFIRM
            } else {
                P1_NON_CONFIRM
            },
            0,
            &derivation_path,
        )?;
        if key.len() != 32 {
            return Err(RemoteWalletError::Protocol("Key packet size mismatch"));
        }
        Ok(Pubkey::new(&key))
    }

    fn sign_message(
        &self,
        derivation_path: &DerivationPath,
        data: &[u8],
    ) -> Result<Signature, RemoteWalletError> {
        let mut payload = if self.outdated_app() {
            extend_and_serialize(derivation_path)
        } else {
            extend_and_serialize_multiple(&[derivation_path])
        };
        if data.len() > u16::max_value() as usize {
            return Err(RemoteWalletError::InvalidInput(
                "Message to sign is too long".to_string(),
            ));
        }

        // Check to see if this data needs to be split up and
        // sent in chunks.
        let max_size = MAX_CHUNK_SIZE - payload.len();
        let empty = vec![];
        let (data, remaining_data) = if data.len() > max_size {
            data.split_at(max_size)
        } else {
            (data, empty.as_ref())
        };

        // Pack the first chunk
        if self.outdated_app() {
            for byte in (data.len() as u16).to_be_bytes().iter() {
                payload.push(*byte);
            }
        }
        payload.extend_from_slice(data);
        trace!("Serialized payload length {:?}", payload.len());

        let p2 = if remaining_data.is_empty() {
            0
        } else {
            P2_MORE
        };

        let p1 = P1_CONFIRM;
        let mut result = self.send_apdu(
            if self.outdated_app() {
                commands::DEPRECATED_SIGN_MESSAGE
            } else {
                commands::SIGN_MESSAGE
            },
            p1,
            p2,
            &payload,
        )?;

        // Pack and send the remaining chunks
        if !remaining_data.is_empty() {
            let mut chunks: Vec<_> = remaining_data
                .chunks(MAX_CHUNK_SIZE)
                .map(|data| {
                    let mut payload = if self.outdated_app() {
                        (data.len() as u16).to_be_bytes().to_vec()
                    } else {
                        vec![]
                    };
                    payload.extend_from_slice(data);
                    let p2 = P2_EXTEND | P2_MORE;
                    (p2, payload)
                })
                .collect();

            // Clear the P2_MORE bit on the last item.
            chunks.last_mut().unwrap().0 &= !P2_MORE;

            for (p2, payload) in chunks {
                result = self.send_apdu(commands::SIGN_MESSAGE, p1, p2, &payload)?;
            }
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
pub fn is_valid_ledger(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == LEDGER_VID
        && (LEDGER_NANO_S_PIDS.contains(&product_id) || LEDGER_NANO_X_PIDS.contains(&product_id))
}

/// Build the derivation path byte array from a DerivationPath selection
fn extend_and_serialize(derivation_path: &DerivationPath) -> Vec<u8> {
    let byte = if derivation_path.change.is_some() {
        4
    } else if derivation_path.account.is_some() {
        3
    } else {
        2
    };
    let mut concat_derivation = vec![byte];
    concat_derivation.extend_from_slice(&SOL_DERIVATION_PATH_BE);
    if let Some(account) = derivation_path.account {
        let hardened_account = account | HARDENED_BIT;
        concat_derivation.extend_from_slice(&hardened_account.to_be_bytes());
        if let Some(change) = derivation_path.change {
            let hardened_change = change | HARDENED_BIT;
            concat_derivation.extend_from_slice(&hardened_change.to_be_bytes());
        }
    }
    concat_derivation
}

fn extend_and_serialize_multiple(derivation_paths: &[&DerivationPath]) -> Vec<u8> {
    let mut concat_derivation = vec![derivation_paths.len() as u8];
    for derivation_path in derivation_paths {
        concat_derivation.append(&mut extend_and_serialize(derivation_path));
    }
    concat_derivation
}

/// Choose a Ledger wallet based on matching info fields
pub fn get_ledger_from_info(
    info: RemoteWalletInfo,
    keypair_name: &str,
    wallet_manager: &RemoteWalletManager,
) -> Result<Arc<LedgerWallet>, RemoteWalletError> {
    let devices = wallet_manager.list_devices();
    let mut matches = devices
        .iter()
        .filter(|&device_info| device_info.matches(&info));
    if matches
        .clone()
        .all(|device_info| device_info.error.is_some())
    {
        let first_device = matches.next();
        if let Some(device) = first_device {
            return Err(device.error.clone().unwrap());
        }
    }
    let mut matches: Vec<(Pubkey, String)> = matches
        .filter(|&device_info| device_info.error.is_none())
        .map(|device_info| (device_info.pubkey, device_info.get_pretty_path()))
        .collect();
    if matches.is_empty() {
        return Err(RemoteWalletError::NoDeviceFound);
    }
    matches.sort_by(|a, b| a.1.cmp(&b.1));
    let (pubkeys, device_paths): (Vec<Pubkey>, Vec<String>) = matches.into_iter().unzip();

    let wallet_base_pubkey = if pubkeys.len() > 1 {
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(&format!(
                "Multiple hardware wallets found. Please select a device for {:?}",
                keypair_name
            ))
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

//
fn is_last_part(p2: u8) -> bool {
    p2 & P2_MORE == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_last_part() {
        // Bytes with bit-2 set to 0 should return true
        assert!(is_last_part(0b00));
        assert!(is_last_part(0b01));
        assert!(is_last_part(0b101));
        assert!(is_last_part(0b1001));
        assert!(is_last_part(0b1101));

        // Bytes with bit-2 set to 1 should return false
        assert!(!is_last_part(0b10));
        assert!(!is_last_part(0b11));
        assert!(!is_last_part(0b110));
        assert!(!is_last_part(0b111));
        assert!(!is_last_part(0b1010));

        // Test implementation-specific uses
        let p2 = 0;
        assert!(is_last_part(p2));
        let p2 = P2_EXTEND | P2_MORE;
        assert!(!is_last_part(p2));
        assert!(is_last_part(p2 & !P2_MORE));
    }

    #[test]
    fn test_parse_status() {
        assert_eq!(LedgerWallet::parse_status(APDU_SUCCESS_CODE).unwrap(), ());
        if let RemoteWalletError::LedgerError(err) = LedgerWallet::parse_status(0x6985).unwrap_err()
        {
            assert_eq!(err, LedgerError::UserCancel);
        }
        if let RemoteWalletError::Protocol(err) = LedgerWallet::parse_status(0x6fff).unwrap_err() {
            assert_eq!(err, "Unknown error");
        }
    }
}
