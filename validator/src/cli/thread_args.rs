//! Arguments for controlling the number of threads allocated for various tasks

use {
    clap::{value_t_or_exit, Arg, ArgMatches},
    solana_clap_utils::{hidden_unless_forced, input_validators::is_within_range},
    solana_rayon_threadlimit::get_max_thread_count,
    std::{num::NonZeroUsize, ops::RangeInclusive},
};

// Need this struct to provide &str whose lifetime matches that of the CLAP Arg's
pub struct DefaultThreadArgs {
    pub ip_echo_server_threads: String,
    pub replay_forks_threads: String,
    pub replay_transactions_threads: String,
    pub tvu_receive_threads: String,
}

impl Default for DefaultThreadArgs {
    fn default() -> Self {
        Self {
            ip_echo_server_threads: IpEchoServerThreadsArg::bounded_default().to_string(),
            replay_forks_threads: ReplayForksThreadsArg::bounded_default().to_string(),
            replay_transactions_threads: ReplayTransactionsThreadsArg::bounded_default()
                .to_string(),
            tvu_receive_threads: TvuReceiveThreadsArg::bounded_default().to_string(),
        }
    }
}

pub fn thread_args<'a>(defaults: &DefaultThreadArgs) -> Vec<Arg<'_, 'a>> {
    vec![
        new_thread_arg::<IpEchoServerThreadsArg>(&defaults.ip_echo_server_threads),
        new_thread_arg::<ReplayForksThreadsArg>(&defaults.replay_forks_threads),
        new_thread_arg::<ReplayTransactionsThreadsArg>(&defaults.replay_transactions_threads),
        new_thread_arg::<TvuReceiveThreadsArg>(&defaults.tvu_receive_threads),
    ]
}

fn new_thread_arg<'a, T: ThreadArg>(default: &str) -> Arg<'_, 'a> {
    Arg::with_name(T::NAME)
        .long(T::LONG_NAME)
        .takes_value(true)
        .value_name("NUMBER")
        .default_value(default)
        .validator(|num| is_within_range(num, T::range()))
        .hidden(hidden_unless_forced())
        .help(T::HELP)
}

pub struct NumThreadConfig {
    pub ip_echo_server_threads: NonZeroUsize,
    pub replay_forks_threads: NonZeroUsize,
    pub replay_transactions_threads: NonZeroUsize,
    pub tvu_receive_threads: NonZeroUsize,
}

pub fn parse_num_threads_args(matches: &ArgMatches) -> NumThreadConfig {
    NumThreadConfig {
        ip_echo_server_threads: value_t_or_exit!(
            matches,
            IpEchoServerThreadsArg::NAME,
            NonZeroUsize
        ),
        replay_forks_threads: if matches.is_present("replay_slots_concurrently") {
            NonZeroUsize::new(4).expect("4 is non-zero")
        } else {
            value_t_or_exit!(matches, ReplayForksThreadsArg::NAME, NonZeroUsize)
        },
        replay_transactions_threads: value_t_or_exit!(
            matches,
            ReplayTransactionsThreadsArg::NAME,
            NonZeroUsize
        ),
        tvu_receive_threads: value_t_or_exit!(matches, TvuReceiveThreadsArg::NAME, NonZeroUsize),
    }
}

/// Configuration for CLAP arguments that control the number of threads for various functions
trait ThreadArg {
    /// The argument's name
    const NAME: &'static str;
    /// The argument's long name
    const LONG_NAME: &'static str;
    /// The argument's help message
    const HELP: &'static str;

    /// The default number of threads
    fn default() -> usize;
    /// The default number of threads, bounded by Self::max()
    /// This prevents potential CLAP issues on low core count machines where
    /// a fixed value in Self::default() could be greater than Self::max()
    fn bounded_default() -> usize {
        std::cmp::min(Self::default(), Self::max())
    }
    /// The minimum allowed number of threads (inclusive)
    fn min() -> usize {
        1
    }
    /// The maximum allowed number of threads (inclusive)
    fn max() -> usize {
        // By default, no thread pool should scale over the number of the machine's threads
        get_max_thread_count()
    }
    /// The range of allowed number of threads (inclusive on both ends)
    fn range() -> RangeInclusive<usize> {
        RangeInclusive::new(Self::min(), Self::max())
    }
}

struct IpEchoServerThreadsArg;
impl ThreadArg for IpEchoServerThreadsArg {
    const NAME: &'static str = "ip_echo_server_threads";
    const LONG_NAME: &'static str = "ip-echo-server-threads";
    const HELP: &'static str = "Number of threads to use for the IP echo server";

    fn default() -> usize {
        solana_net_utils::DEFAULT_IP_ECHO_SERVER_THREADS.get()
    }
    fn min() -> usize {
        solana_net_utils::MINIMUM_IP_ECHO_SERVER_THREADS.get()
    }
}

struct ReplayForksThreadsArg;
impl ThreadArg for ReplayForksThreadsArg {
    const NAME: &'static str = "replay_forks_threads";
    const LONG_NAME: &'static str = "replay-forks-threads";
    const HELP: &'static str = "Number of threads to use for replay of blocks on different forks";

    fn default() -> usize {
        // Default to single threaded fork execution
        1
    }
    fn max() -> usize {
        // Choose a value that is small enough to limit the overhead of having a large thread pool
        // while also being large enough to allow replay of all active forks in most scenarios
        4
    }
}

struct ReplayTransactionsThreadsArg;
impl ThreadArg for ReplayTransactionsThreadsArg {
    const NAME: &'static str = "replay_transactions_threads";
    const LONG_NAME: &'static str = "replay-transactions-threads";
    const HELP: &'static str = "Number of threads to use for transaction replay";

    fn default() -> usize {
        get_max_thread_count()
    }
}

struct TvuReceiveThreadsArg;
impl ThreadArg for TvuReceiveThreadsArg {
    const NAME: &'static str = "tvu_receive_threads";
    const LONG_NAME: &'static str = "tvu-receive-threads";
    const HELP: &'static str =
        "Number of threads (and sockets) to use for receiving shreds on the TVU port";

    fn default() -> usize {
        solana_gossip::cluster_info::DEFAULT_NUM_TVU_SOCKETS.get()
    }
    fn min() -> usize {
        solana_gossip::cluster_info::MINIMUM_NUM_TVU_SOCKETS.get()
    }
}
