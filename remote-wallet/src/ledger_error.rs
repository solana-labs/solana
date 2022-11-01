use {num_derive::FromPrimitive, thiserror::Error};

#[derive(Error, Debug, Clone, FromPrimitive, PartialEq, Eq)]
pub enum LedgerError {
    #[error("Solana app not open on Ledger device")]
    NoAppResponse = 0x6700,

    #[error("Ledger sdk exception")]
    SdkException = 0x6801,

    #[error("Ledger invalid parameter")]
    SdkInvalidParameter = 0x6802,

    #[error("Ledger overflow")]
    SdkExceptionOverflow = 0x6803,

    #[error("Ledger security exception")]
    SdkExceptionSecurity = 0x6804,

    #[error("Ledger invalid CRC")]
    SdkInvalidCrc = 0x6805,

    #[error("Ledger invalid checksum")]
    SdkInvalidChecksum = 0x6806,

    #[error("Ledger invalid counter")]
    SdkInvalidCounter = 0x6807,

    #[error("Ledger operation not supported")]
    SdkNotSupported = 0x6808,

    #[error("Ledger invalid state")]
    SdkInvalidState = 0x6809,

    #[error("Ledger timeout")]
    SdkTimeout = 0x6810,

    #[error("Ledger PIC exception")]
    SdkExceptionPic = 0x6811,

    #[error("Ledger app exit exception")]
    SdkExceptionAppExit = 0x6812,

    #[error("Ledger IO overflow exception")]
    SdkExceptionIoOverflow = 0x6813,

    #[error("Ledger IO header exception")]
    SdkExceptionIoHeader = 0x6814,

    #[error("Ledger IO state exception")]
    SdkExceptionIoState = 0x6815,

    #[error("Ledger IO reset exception")]
    SdkExceptionIoReset = 0x6816,

    #[error("Ledger CX port exception")]
    SdkExceptionCxPort = 0x6817,

    #[error("Ledger system exception")]
    SdkExceptionSystem = 0x6818,

    #[error("Ledger out of space")]
    SdkNotEnoughSpace = 0x6819,

    #[error("Ledger invalid counter")]
    NoApduReceived = 0x6982,

    #[error("Ledger operation rejected by the user")]
    UserCancel = 0x6985,

    #[error("Ledger received invalid Solana message")]
    SolanaInvalidMessage = 0x6a80,

    #[error("Ledger received message with invalid header")]
    SolanaInvalidMessageHeader = 0x6a81,

    #[error("Ledger received message in invalid format")]
    SolanaInvalidMessageFormat = 0x6a82,

    #[error("Ledger received message with invalid size")]
    SolanaInvalidMessageSize = 0x6a83,

    #[error("Solana summary finalization failed on Ledger device")]
    SolanaSummaryFinalizeFailed = 0x6f00,

    #[error("Solana summary update failed on Ledger device")]
    SolanaSummaryUpdateFailed = 0x6f01,

    #[error("Ledger received unimplemented instruction")]
    UnimplementedInstruction = 0x6d00,

    #[error("Ledger received invalid CLA")]
    InvalidCla = 0x6e00,
}
