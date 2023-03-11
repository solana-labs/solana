use {
    super::DataSetFirstShred,
    rayon,
    solana_sdk::clock::Slot,
    std::{io, time::Duration},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum StartError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    RayonBuildPool(#[from] rayon::ThreadPoolBuildError),
}

#[derive(Debug, Error)]
pub enum AddDataSetError {
    #[error("Service is partially shutdown.  Worker thread already terminated")]
    WorkerTerminated,
}

#[derive(Debug, Error)]
pub enum WaitForResultError {
    #[error("No results for {slot}:{data_set} in {waited_for:?}")]
    Timeout {
        slot: Slot,
        data_set: DataSetFirstShred,
        waited_for: Duration,
    },
    #[error("Service is partially shutdown.  Worker thread already terminated")]
    WorkerTerminated,
}

#[derive(Debug, Error)]
pub enum CleanupError {
    #[error("Service is partially shutdown.  Worker thread already terminated")]
    WorkerTerminated,
}

// We normally want to both print errors and report them in metrics.
// For release builds, this will report an error and continue.
// For debug build, that is in tests, it will panic instead.
macro_rules! unexpected_error {
    (
        $context:literal,
        $counter:literal,
        $message:literal,
        $(
            $details:literal,
            $( $args:expr ),*
        )?
        $(,)?
    ) => {
        if cfg!(debug_assertions) {
            panic!(
                concat!(
                    "entry-verification-service: ",
                    $context,
                    ": ",
                    $message,
                    ".",
                    $(
                        "\n",
                        $details
                    )?
                ),
                $(
                    $( $args ),*
                )?
            );
        } else {
            warn!(
                concat!(
                    "entry-verification-service: ",
                    $context,
                    ": ",
                    $message,
                    ".",
                    $(
                        "\n",
                        $details
                    )?
                ),
                $(
                    $( $args ),*
                )?
            );
            datapoint_error!(
                concat!(
                    "entry_verification_service__",
                    $context,
                    "__",
                    $counter
                ),
                $(
                    (
                        "error",
                        format!($details, $( $args ),*),
                        String
                    )
                )?
            );
        }
    };
}
