#[cfg(target_arch = "x86")]
use core::arch::x86::{CpuidResult, __cpuid, __cpuid_count, __get_cpuid_max};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{CpuidResult, __cpuid, __cpuid_count, __get_cpuid_max};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use num_enum::{IntoPrimitive, TryFromPrimitive};
#[cfg(target_os = "linux")]
use std::{fs::File, io::BufReader};
use {
    solana_sdk::timing::AtomicInterval,
    std::{
        collections::HashMap,
        io::BufRead,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
    sys_info::{Error, LoadAvg},
};

const MS_PER_S: u64 = 1_000;
const MS_PER_M: u64 = MS_PER_S * 60;
const MS_PER_H: u64 = MS_PER_M * 60;
const SAMPLE_INTERVAL_UDP_MS: u64 = 2 * MS_PER_S;
const SAMPLE_INTERVAL_OS_NETWORK_LIMITS_MS: u64 = MS_PER_H;
const SAMPLE_INTERVAL_MEM_MS: u64 = 5 * MS_PER_S;
const SAMPLE_INTERVAL_CPU_MS: u64 = 10 * MS_PER_S;
const SAMPLE_INTERVAL_DISK_MS: u64 = 5 * MS_PER_S;
const SLEEP_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(target_os = "linux")]
const PROC_NET_SNMP_PATH: &str = "/proc/net/snmp";
#[cfg(target_os = "linux")]
const PROC_NET_DEV_PATH: &str = "/proc/net/dev";
#[cfg(target_os = "linux")]
const SYS_BLOCK_PATH: &str = "/sys/block";

pub struct SystemMonitorService {
    thread_hdl: JoinHandle<()>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct UdpStats {
    in_datagrams: u64,
    no_ports: u64,
    in_errors: u64,
    out_datagrams: u64,
    rcvbuf_errors: u64,
    sndbuf_errors: u64,
    in_csum_errors: u64,
    ignored_multi: u64,
}

#[derive(Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
// These stats are aggregated across all network devices excluding the loopback interface.
struct NetDevStats {
    // Number of bytes received
    rx_bytes: u64,
    // Number of packets received
    rx_packets: u64,
    // Number of receive errors detected by device driver
    rx_errs: u64,
    // Number of receive packets dropped by the device driver (not included in error count)
    rx_drops: u64,
    // Number of receive FIFO buffer errors
    rx_fifo: u64,
    // Number of receive packet framing errors
    rx_frame: u64,
    // Number of compressed packets received
    rx_compressed: u64,
    // Number of multicast frames received by device driver
    rx_multicast: u64,
    // Number of bytes transmitted
    tx_bytes: u64,
    // Number of packets transmitted
    tx_packets: u64,
    // Number of transmit errors detected by device driver
    tx_errs: u64,
    // Number of transmit packets dropped by device driver
    tx_drops: u64,
    // Number of transmit FIFO buffer errors
    tx_fifo: u64,
    // Number of transmit collisions detected
    tx_colls: u64,
    // Number of transmit carrier losses detected by device driver
    tx_carrier: u64,
    // Number of compressed packets transmitted
    tx_compressed: u64,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct NetStats {
    udp_stats: UdpStats,
    net_dev_stats: NetDevStats,
}

struct CpuInfo {
    cpu_num: u32,
    cpu_freq_mhz: u64,
    load_avg: LoadAvg,
    num_threads: u64,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[derive(IntoPrimitive)]
#[repr(i64)]
enum CpuManufacturer {
    Other,
    Intel,
    Amd,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[derive(IntoPrimitive, TryFromPrimitive, PartialEq, PartialOrd)]
#[repr(u32)]
// The value passed into cpuid via eax, to control what the result means
enum CpuidParamValue {
    Manufacturer = 0,
    Processor = 1,
    Cache = 2,
    SerialNumber = 3,
    Topology = 4,
    Unsupported = 5,
    ThermalAndPower = 6,
    Extended = 7,
}
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const CPUID_PARAM_MAX_SUPPORTED_VALUE: u32 = 7;

#[derive(Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
// These stats are aggregated across all storage devices excluding internal loopbacks.
// Fields are cumulative since boot with the exception of 'num_disks' and 'io_in_progress'
struct DiskStats {
    reads_completed: u64,
    reads_merged: u64,
    sectors_read: u64,
    time_reading_ms: u64,
    writes_completed: u64,
    writes_merged: u64,
    sectors_written: u64,
    time_writing_ms: u64,
    io_in_progress: u64,
    time_io_ms: u64,
    // weighted time multiplies time performing IO by number of commands in the queue
    time_io_weighted_ms: u64,
    discards_completed: u64,
    discards_merged: u64,
    sectors_discarded: u64,
    time_discarding: u64,
    flushes_completed: u64,
    time_flushing: u64,
    num_disks: u64,
}

impl UdpStats {
    fn from_map(udp_stats: &HashMap<String, u64>) -> Self {
        Self {
            in_datagrams: *udp_stats.get("InDatagrams").unwrap_or(&0),
            no_ports: *udp_stats.get("NoPorts").unwrap_or(&0),
            in_errors: *udp_stats.get("InErrors").unwrap_or(&0),
            out_datagrams: *udp_stats.get("OutDatagrams").unwrap_or(&0),
            rcvbuf_errors: *udp_stats.get("RcvbufErrors").unwrap_or(&0),
            sndbuf_errors: *udp_stats.get("SndbufErrors").unwrap_or(&0),
            in_csum_errors: *udp_stats.get("InCsumErrors").unwrap_or(&0),
            ignored_multi: *udp_stats.get("IgnoredMulti").unwrap_or(&0),
        }
    }
}

impl DiskStats {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn accumulate(&mut self, other: &DiskStats) {
        self.reads_completed += other.reads_completed;
        self.reads_merged += other.reads_merged;
        self.sectors_read += other.sectors_read;
        self.time_reading_ms += other.time_reading_ms;
        self.writes_completed += other.writes_completed;
        self.writes_merged += other.writes_merged;
        self.sectors_written += other.sectors_written;
        self.time_writing_ms += other.time_writing_ms;
        self.io_in_progress += other.io_in_progress;
        self.time_io_ms += other.time_io_ms;
        self.time_io_weighted_ms += other.time_io_weighted_ms;
        self.discards_completed += other.discards_completed;
        self.discards_merged += other.discards_merged;
        self.sectors_discarded += other.sectors_discarded;
        self.time_discarding += other.time_discarding;
        self.flushes_completed += other.flushes_completed;
        self.time_flushing += other.time_flushing;
    }
}

fn platform_id() -> String {
    format!(
        "{}/{}/{}",
        std::env::consts::FAMILY,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(target_os = "linux")]
fn read_net_stats() -> Result<NetStats, String> {
    let file_path_snmp = PROC_NET_SNMP_PATH;
    let file_snmp = File::open(file_path_snmp).map_err(|e| e.to_string())?;
    let mut reader_snmp = BufReader::new(file_snmp);

    let file_path_dev = PROC_NET_DEV_PATH;
    let file_dev = File::open(file_path_dev).map_err(|e| e.to_string())?;
    let mut reader_dev = BufReader::new(file_dev);

    let udp_stats = parse_udp_stats(&mut reader_snmp)?;
    let net_dev_stats = parse_net_dev_stats(&mut reader_dev)?;
    Ok(NetStats {
        udp_stats,
        net_dev_stats,
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_udp_stats(reader_snmp: &mut impl BufRead) -> Result<UdpStats, String> {
    let mut udp_lines = Vec::default();
    for line in reader_snmp.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.starts_with("Udp:") {
            udp_lines.push(line);
            if udp_lines.len() == 2 {
                break;
            }
        }
    }
    if udp_lines.len() != 2 {
        return Err(format!(
            "parse error, expected 2 lines, num lines: {}",
            udp_lines.len()
        ));
    }

    let pairs: Vec<_> = udp_lines[0]
        .split_ascii_whitespace()
        .zip(udp_lines[1].split_ascii_whitespace())
        .collect();
    let udp_stats: HashMap<String, u64> = pairs[1..]
        .iter()
        .map(|(label, val)| (label.to_string(), val.parse::<u64>().unwrap()))
        .collect();

    let stats = UdpStats::from_map(&udp_stats);
    Ok(stats)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_net_dev_stats(reader_dev: &mut impl BufRead) -> Result<NetDevStats, String> {
    let mut stats = NetDevStats::default();
    for (line_number, line) in reader_dev.lines().enumerate() {
        if line_number < 2 {
            // Skip first two lines with header information.
            continue;
        }

        let line = line.map_err(|e| e.to_string())?;
        let values: Vec<_> = line.split_ascii_whitespace().collect();

        if values.len() != 17 {
            return Err("parse error, expected exactly 17 stat elements".to_string());
        }
        if values[0] == "lo:" {
            // Filter out the loopback network interface as we are only concerned with
            // external traffic.
            continue;
        }

        stats.rx_bytes += values[1].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_packets += values[2].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_errs += values[3].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_drops += values[4].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_fifo += values[5].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_frame += values[6].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_compressed += values[7].parse::<u64>().map_err(|e| e.to_string())?;
        stats.rx_multicast += values[8].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_bytes += values[9].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_packets += values[10].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_errs += values[11].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_drops += values[12].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_fifo += values[13].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_colls += values[14].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_carrier += values[15].parse::<u64>().map_err(|e| e.to_string())?;
        stats.tx_compressed += values[16].parse::<u64>().map_err(|e| e.to_string())?;
    }

    Ok(stats)
}

#[cfg(target_os = "linux")]
pub fn verify_net_stats_access() -> Result<(), String> {
    read_net_stats()?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn verify_net_stats_access() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_disk_stats() -> Result<DiskStats, String> {
    let mut stats = DiskStats::default();
    let mut num_disks = 0;
    let blk_device_dir_iter = std::fs::read_dir(SYS_BLOCK_PATH).map_err(|e| e.to_string())?;
    blk_device_dir_iter
        .filter_map(|blk_device_dir| {
            match blk_device_dir {
                Ok(blk_device_dir) => {
                    let blk_device_dir_name = &blk_device_dir.file_name();
                    let blk_device_dir_name = blk_device_dir_name.to_string_lossy();
                    if blk_device_dir_name.starts_with("loop")
                        || blk_device_dir_name.starts_with("dm")
                        || blk_device_dir_name.starts_with("md")
                    {
                        // Filter out loopback devices, dmcrypt volumes, and mdraid volumes
                        return None;
                    }
                    let mut path = blk_device_dir.path();
                    path.push("stat");
                    match File::open(path) {
                        Ok(file_diskstats) => Some(file_diskstats),
                        Err(_) => None,
                    }
                }
                Err(_) => None,
            }
        })
        .for_each(|file_diskstats| {
            let mut reader_diskstats = BufReader::new(file_diskstats);
            stats.accumulate(&parse_disk_stats(&mut reader_diskstats).unwrap_or_default());
            num_disks += 1;
        });
    stats.num_disks = num_disks;
    Ok(stats)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_disk_stats(reader_diskstats: &mut impl BufRead) -> Result<DiskStats, String> {
    let mut stats = DiskStats::default();
    let mut line = String::new();
    reader_diskstats
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let values: Vec<_> = line.split_ascii_whitespace().collect();
    let num_elements = values.len();

    if num_elements != 11 && num_elements != 15 && num_elements != 17 {
        return Err("parse error, unknown number of disk stat elements".to_string());
    }

    stats.reads_completed = values[0].parse::<u64>().map_err(|e| e.to_string())?;
    stats.reads_merged = values[1].parse::<u64>().map_err(|e| e.to_string())?;
    stats.sectors_read = values[2].parse::<u64>().map_err(|e| e.to_string())?;
    stats.time_reading_ms = values[3].parse::<u64>().map_err(|e| e.to_string())?;
    stats.writes_completed = values[4].parse::<u64>().map_err(|e| e.to_string())?;
    stats.writes_merged = values[5].parse::<u64>().map_err(|e| e.to_string())?;
    stats.sectors_written = values[6].parse::<u64>().map_err(|e| e.to_string())?;
    stats.time_writing_ms = values[7].parse::<u64>().map_err(|e| e.to_string())?;
    stats.io_in_progress = values[8].parse::<u64>().map_err(|e| e.to_string())?;
    stats.time_io_ms = values[9].parse::<u64>().map_err(|e| e.to_string())?;
    stats.time_io_weighted_ms = values[10].parse::<u64>().map_err(|e| e.to_string())?;
    if num_elements > 11 {
        // Kernel 4.18+ appends four more fields for discard
        stats.discards_completed = values[11].parse::<u64>().map_err(|e| e.to_string())?;
        stats.discards_merged = values[12].parse::<u64>().map_err(|e| e.to_string())?;
        stats.sectors_discarded = values[13].parse::<u64>().map_err(|e| e.to_string())?;
        stats.time_discarding = values[14].parse::<u64>().map_err(|e| e.to_string())?;
    }
    if num_elements > 15 {
        // Kernel 5.5+ appends two more fields for flush requests
        stats.flushes_completed = values[15].parse::<u64>().map_err(|e| e.to_string())?;
        stats.time_flushing = values[16].parse::<u64>().map_err(|e| e.to_string())?;
    }

    Ok(stats)
}

pub struct SystemMonitorStatsReportConfig {
    pub report_os_memory_stats: bool,
    pub report_os_network_stats: bool,
    pub report_os_cpu_stats: bool,
    pub report_os_disk_stats: bool,
}

impl SystemMonitorService {
    pub fn new(exit: Arc<AtomicBool>, config: SystemMonitorStatsReportConfig) -> Self {
        info!("Starting SystemMonitorService");
        let thread_hdl = Builder::new()
            .name("solSystemMonitr".to_string())
            .spawn(move || {
                Self::run(exit, config);
            })
            .unwrap();

        Self { thread_hdl }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn linux_get_recommended_network_limits() -> HashMap<&'static str, i64> {
        // Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
        let mut recommended_limits: HashMap<&str, i64> = HashMap::default();
        recommended_limits.insert("net.core.rmem_max", 134217728);
        recommended_limits.insert("net.core.rmem_default", 134217728);
        recommended_limits.insert("net.core.wmem_max", 134217728);
        recommended_limits.insert("net.core.wmem_default", 134217728);
        recommended_limits.insert("vm.max_map_count", 1000000);

        // Additionally collect the following limits
        recommended_limits.insert("net.core.optmem_max", 0);
        recommended_limits.insert("net.core.netdev_max_backlog", 0);

        recommended_limits
    }

    #[cfg(target_os = "linux")]
    fn linux_get_current_network_limits(
        recommended_limits: &HashMap<&'static str, i64>,
    ) -> HashMap<&'static str, i64> {
        use sysctl::Sysctl;

        fn sysctl_read(name: &str) -> Result<String, sysctl::SysctlError> {
            let ctl = sysctl::Ctl::new(name)?;
            let val = ctl.value_string()?;
            Ok(val)
        }

        let mut current_limits: HashMap<&str, i64> = HashMap::default();
        for (key, _) in recommended_limits.iter() {
            let current_val = match sysctl_read(key) {
                Ok(val) => val.parse::<i64>().unwrap(),
                Err(e) => {
                    error!("Failed to query value for {}: {}", key, e);
                    -1
                }
            };
            current_limits.insert(key, current_val);
        }
        current_limits
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn linux_report_network_limits(
        current_limits: &HashMap<&str, i64>,
        recommended_limits: &HashMap<&'static str, i64>,
    ) -> bool {
        let mut check_failed = false;
        for (key, recommended_val) in recommended_limits.iter() {
            let current_val = *current_limits.get(key).unwrap_or(&-1);
            if current_val < *recommended_val {
                datapoint_warn!("os-config", (key, current_val, i64));
                warn!(
                    "  {}: recommended={} current={}, too small",
                    key, recommended_val, current_val
                );
                check_failed = true;
            } else {
                datapoint_info!("os-config", (key, current_val, i64));
                info!(
                    "  {}: recommended={} current={}",
                    key, recommended_val, current_val
                );
            }
        }
        if check_failed {
            datapoint_warn!("os-config", ("network_limit_test_failed", 1, i64));
        }
        !check_failed
    }

    #[cfg(not(target_os = "linux"))]
    pub fn check_os_network_limits() -> bool {
        datapoint_info!("os-config", ("platform", platform_id(), String));
        true
    }

    #[cfg(target_os = "linux")]
    pub fn check_os_network_limits() -> bool {
        datapoint_info!("os-config", ("platform", platform_id(), String));
        let recommended_limits = Self::linux_get_recommended_network_limits();
        let current_limits = Self::linux_get_current_network_limits(&recommended_limits);
        Self::linux_report_network_limits(&current_limits, &recommended_limits)
    }

    #[cfg(target_os = "linux")]
    fn process_net_stats(net_stats: &mut Option<NetStats>) {
        match read_net_stats() {
            Ok(new_stats) => {
                if let Some(old_stats) = net_stats {
                    Self::report_net_stats(old_stats, &new_stats);
                }
                *net_stats = Some(new_stats);
            }
            Err(e) => warn!("read_net_stats: {}", e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn process_net_stats(_net_stats: &mut Option<NetStats>) {}

    #[cfg(target_os = "linux")]
    fn report_net_stats(old_stats: &NetStats, new_stats: &NetStats) {
        datapoint_info!(
            "net-stats-validator",
            (
                "in_datagrams_delta",
                new_stats.udp_stats.in_datagrams - old_stats.udp_stats.in_datagrams,
                i64
            ),
            (
                "no_ports_delta",
                new_stats.udp_stats.no_ports - old_stats.udp_stats.no_ports,
                i64
            ),
            (
                "in_errors_delta",
                new_stats.udp_stats.in_errors - old_stats.udp_stats.in_errors,
                i64
            ),
            (
                "out_datagrams_delta",
                new_stats.udp_stats.out_datagrams - old_stats.udp_stats.out_datagrams,
                i64
            ),
            (
                "rcvbuf_errors_delta",
                new_stats.udp_stats.rcvbuf_errors - old_stats.udp_stats.rcvbuf_errors,
                i64
            ),
            (
                "sndbuf_errors_delta",
                new_stats.udp_stats.sndbuf_errors - old_stats.udp_stats.sndbuf_errors,
                i64
            ),
            (
                "in_csum_errors_delta",
                new_stats.udp_stats.in_csum_errors - old_stats.udp_stats.in_csum_errors,
                i64
            ),
            (
                "ignored_multi_delta",
                new_stats.udp_stats.ignored_multi - old_stats.udp_stats.ignored_multi,
                i64
            ),
            ("in_errors", new_stats.udp_stats.in_errors, i64),
            ("rcvbuf_errors", new_stats.udp_stats.rcvbuf_errors, i64),
            ("sndbuf_errors", new_stats.udp_stats.sndbuf_errors, i64),
            (
                "rx_bytes_delta",
                new_stats
                    .net_dev_stats
                    .rx_bytes
                    .saturating_sub(old_stats.net_dev_stats.rx_bytes),
                i64
            ),
            (
                "rx_packets_delta",
                new_stats
                    .net_dev_stats
                    .rx_packets
                    .saturating_sub(old_stats.net_dev_stats.rx_packets),
                i64
            ),
            (
                "rx_errs_delta",
                new_stats
                    .net_dev_stats
                    .rx_errs
                    .saturating_sub(old_stats.net_dev_stats.rx_errs),
                i64
            ),
            (
                "rx_drops_delta",
                new_stats
                    .net_dev_stats
                    .rx_drops
                    .saturating_sub(old_stats.net_dev_stats.rx_drops),
                i64
            ),
            (
                "rx_fifo_delta",
                new_stats
                    .net_dev_stats
                    .rx_fifo
                    .saturating_sub(old_stats.net_dev_stats.rx_fifo),
                i64
            ),
            (
                "rx_frame_delta",
                new_stats
                    .net_dev_stats
                    .rx_frame
                    .saturating_sub(old_stats.net_dev_stats.rx_frame),
                i64
            ),
            (
                "tx_bytes_delta",
                new_stats
                    .net_dev_stats
                    .tx_bytes
                    .saturating_sub(old_stats.net_dev_stats.tx_bytes),
                i64
            ),
            (
                "tx_packets_delta",
                new_stats
                    .net_dev_stats
                    .tx_packets
                    .saturating_sub(old_stats.net_dev_stats.tx_packets),
                i64
            ),
            (
                "tx_errs_delta",
                new_stats
                    .net_dev_stats
                    .tx_errs
                    .saturating_sub(old_stats.net_dev_stats.tx_errs),
                i64
            ),
            (
                "tx_drops_delta",
                new_stats
                    .net_dev_stats
                    .tx_drops
                    .saturating_sub(old_stats.net_dev_stats.tx_drops),
                i64
            ),
            (
                "tx_fifo_delta",
                new_stats
                    .net_dev_stats
                    .tx_fifo
                    .saturating_sub(old_stats.net_dev_stats.tx_fifo),
                i64
            ),
            (
                "tx_colls_delta",
                new_stats
                    .net_dev_stats
                    .tx_colls
                    .saturating_sub(old_stats.net_dev_stats.tx_colls),
                i64
            ),
        );
    }

    fn calc_percent(numerator: u64, denom: u64) -> f64 {
        if denom == 0 {
            0.0
        } else {
            (numerator as f64 / denom as f64) * 100.0
        }
    }

    fn report_mem_stats() {
        // get mem info (in kb)
        if let Ok(info) = sys_info::mem_info() {
            const KB: u64 = 1_024;
            datapoint_info!(
                "memory-stats",
                ("total", info.total * KB, i64),
                ("swap_total", info.swap_total * KB, i64),
                (
                    "free_percent",
                    Self::calc_percent(info.free, info.total),
                    f64
                ),
                (
                    "used_bytes",
                    info.total.saturating_sub(info.avail) * KB,
                    i64
                ),
                (
                    "avail_percent",
                    Self::calc_percent(info.avail, info.total),
                    f64
                ),
                (
                    "buffers_percent",
                    Self::calc_percent(info.buffers, info.total),
                    f64
                ),
                (
                    "cached_percent",
                    Self::calc_percent(info.cached, info.total),
                    f64
                ),
                (
                    "swap_free_percent",
                    Self::calc_percent(info.swap_free, info.swap_total),
                    f64
                ),
            )
        }
    }

    fn cpu_info() -> Result<CpuInfo, Error> {
        let cpu_num = sys_info::cpu_num()?;
        let cpu_freq_mhz = sys_info::cpu_speed()?;
        let load_avg = sys_info::loadavg()?;
        let num_threads = sys_info::proc_total()?;

        Ok(CpuInfo {
            cpu_num,
            cpu_freq_mhz,
            load_avg,
            num_threads,
        })
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn report_cpuid_values() {
        const CPUID_MANUFACTURER_EBX_INTEL: u32 = 0x756e6547;
        const CPUID_MANUFACTURER_EDX_INTEL: u32 = 0x49656e69;
        const CPUID_MANUFACTURER_ECX_INTEL: u32 = 0x6c65746e;
        const CPUID_MANUFACTURER_EBX_AMD: u32 = 0x68747541;
        const CPUID_MANUFACTURER_EDX_AMD: u32 = 0x69746e65;
        const CPUID_MANUFACTURER_ECX_AMD: u32 = 0x444d4163;

        unsafe {
            let cpuid_mfr = __cpuid(0);
            let cpuid_empty = CpuidResult {
                eax: 0,
                ebx: 0,
                ecx: 0,
                edx: 0,
            };

            let max_leaf = match CpuidParamValue::try_from(std::cmp::min(
                cpuid_mfr.eax,
                CPUID_PARAM_MAX_SUPPORTED_VALUE,
            )) {
                Ok(val) => val,
                Err(_err) => CpuidParamValue::Manufacturer,
            };

            let mfr_id = if cpuid_mfr.ebx == CPUID_MANUFACTURER_EBX_INTEL
                && cpuid_mfr.edx == CPUID_MANUFACTURER_EDX_INTEL
                && cpuid_mfr.ecx == CPUID_MANUFACTURER_ECX_INTEL
            {
                CpuManufacturer::Intel // GenuineIntel
            } else if cpuid_mfr.ebx == CPUID_MANUFACTURER_EBX_AMD
                && cpuid_mfr.edx == CPUID_MANUFACTURER_EDX_AMD
                && cpuid_mfr.ecx == CPUID_MANUFACTURER_ECX_AMD
            {
                CpuManufacturer::Amd // AuthenticAMD
            } else {
                CpuManufacturer::Other // anything else
            };

            let cpuid_processor = if CpuidParamValue::Processor <= max_leaf {
                __cpuid(CpuidParamValue::Processor.into())
            } else {
                cpuid_empty
            };
            let cpuid_cache = if CpuidParamValue::Cache <= max_leaf {
                __cpuid(CpuidParamValue::Cache.into())
            } else {
                cpuid_empty
            };
            let cpuid_topology = if CpuidParamValue::Topology <= max_leaf {
                __cpuid(CpuidParamValue::Topology.into())
            } else {
                cpuid_empty
            };
            let cpuid_extended_0 = if CpuidParamValue::Extended <= max_leaf {
                __cpuid_count(CpuidParamValue::Extended.into(), 0)
            } else {
                cpuid_empty
            };
            let cpuid_extended_1 = if CpuidParamValue::Extended <= max_leaf {
                if 1 <= __get_cpuid_max(CpuidParamValue::Extended.into()).1 {
                    __cpuid_count(CpuidParamValue::Extended.into(), 1)
                } else {
                    cpuid_empty
                }
            } else {
                cpuid_empty
            };

            datapoint_info!(
                "cpuid-values",
                ("manufacturer_id", i64::from(mfr_id), i64),
                ("cpuid_processor_eax", i64::from(cpuid_processor.eax), i64),
                ("cpuid_processor_ebx", i64::from(cpuid_processor.ebx), i64),
                ("cpuid_processor_ecx", i64::from(cpuid_processor.ecx), i64),
                ("cpuid_processor_edx", i64::from(cpuid_processor.edx), i64),
                ("cpuid_cache_eax", i64::from(cpuid_cache.eax), i64),
                ("cpuid_cache_ebx", i64::from(cpuid_cache.ebx), i64),
                ("cpuid_cache_ecx", i64::from(cpuid_cache.ecx), i64),
                ("cpuid_cache_edx", i64::from(cpuid_cache.edx), i64),
                ("cpuid_topology_eax", i64::from(cpuid_topology.eax), i64),
                ("cpuid_topology_ebx", i64::from(cpuid_topology.ebx), i64),
                ("cpuid_topology_ecx", i64::from(cpuid_topology.ecx), i64),
                ("cpuid_topology_edx", i64::from(cpuid_topology.edx), i64),
                ("cpuid_extended_0_ebx", i64::from(cpuid_extended_0.ebx), i64),
                ("cpuid_extended_0_ecx", i64::from(cpuid_extended_0.ecx), i64),
                ("cpuid_extended_0_edx", i64::from(cpuid_extended_0.edx), i64),
                ("cpuid_extended_1_eax", i64::from(cpuid_extended_1.eax), i64),
            );
        };
    }

    fn report_cpu_stats() {
        if let Ok(info) = Self::cpu_info() {
            datapoint_info!(
                "cpu-stats",
                ("cpu_num", info.cpu_num as i64, i64),
                ("cpu0_freq_mhz", info.cpu_freq_mhz as i64, i64),
                ("average_load_one_minute", info.load_avg.one, f64),
                ("average_load_five_minutes", info.load_avg.five, f64),
                ("average_load_fifteen_minutes", info.load_avg.fifteen, f64),
                ("total_num_threads", info.num_threads as i64, i64),
            )
        }

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        Self::report_cpuid_values();
    }

    #[cfg(target_os = "linux")]
    fn process_disk_stats(disk_stats: &mut Option<DiskStats>) {
        match read_disk_stats() {
            Ok(new_stats) => {
                if let Some(old_stats) = disk_stats {
                    Self::report_disk_stats(old_stats, &new_stats);
                }
                *disk_stats = Some(new_stats);
            }
            Err(e) => warn!("read_disk_stats: {}", e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn process_disk_stats(_disk_stats: &mut Option<DiskStats>) {}

    #[cfg(target_os = "linux")]
    fn report_disk_stats(old_stats: &DiskStats, new_stats: &DiskStats) {
        datapoint_info!(
            "disk-stats",
            (
                "reads_completed",
                new_stats
                    .reads_completed
                    .saturating_sub(old_stats.reads_completed),
                i64
            ),
            (
                "reads_merged",
                new_stats
                    .reads_merged
                    .saturating_sub(old_stats.reads_merged),
                i64
            ),
            (
                "sectors_read",
                new_stats
                    .sectors_read
                    .saturating_sub(old_stats.sectors_read),
                i64
            ),
            (
                "time_reading_ms",
                new_stats
                    .time_reading_ms
                    .saturating_sub(old_stats.time_reading_ms),
                i64
            ),
            (
                "writes_completed",
                new_stats
                    .writes_completed
                    .saturating_sub(old_stats.writes_completed),
                i64
            ),
            (
                "writes_merged",
                new_stats
                    .writes_merged
                    .saturating_sub(old_stats.writes_merged),
                i64
            ),
            (
                "sectors_written",
                new_stats
                    .sectors_written
                    .saturating_sub(old_stats.sectors_written),
                i64
            ),
            (
                "time_writing_ms",
                new_stats
                    .time_writing_ms
                    .saturating_sub(old_stats.time_writing_ms),
                i64
            ),
            ("io_in_progress", new_stats.io_in_progress, i64),
            (
                "time_io_ms",
                new_stats.time_io_ms.saturating_sub(old_stats.time_io_ms),
                i64
            ),
            (
                "time_io_weighted_ms",
                new_stats
                    .time_io_weighted_ms
                    .saturating_sub(old_stats.time_io_weighted_ms),
                i64
            ),
            (
                "discards_completed",
                new_stats
                    .discards_completed
                    .saturating_sub(old_stats.discards_completed),
                i64
            ),
            (
                "discards_merged",
                new_stats
                    .discards_merged
                    .saturating_sub(old_stats.discards_merged),
                i64
            ),
            (
                "sectors_discarded",
                new_stats
                    .sectors_discarded
                    .saturating_sub(old_stats.sectors_discarded),
                i64
            ),
            (
                "time_discarding",
                new_stats
                    .time_discarding
                    .saturating_sub(old_stats.time_discarding),
                i64
            ),
            (
                "flushes_completed",
                new_stats
                    .flushes_completed
                    .saturating_sub(old_stats.flushes_completed),
                i64
            ),
            (
                "time_flushing",
                new_stats
                    .time_flushing
                    .saturating_sub(old_stats.time_flushing),
                i64
            ),
            ("num_disks", new_stats.num_disks, i64),
        )
    }

    pub fn run(exit: Arc<AtomicBool>, config: SystemMonitorStatsReportConfig) {
        let mut udp_stats = None;
        let mut disk_stats = None;
        let network_limits_timer = AtomicInterval::default();
        let udp_timer = AtomicInterval::default();
        let mem_timer = AtomicInterval::default();
        let cpu_timer = AtomicInterval::default();
        let disk_timer = AtomicInterval::default();

        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            if config.report_os_network_stats {
                if network_limits_timer.should_update(SAMPLE_INTERVAL_OS_NETWORK_LIMITS_MS) {
                    Self::check_os_network_limits();
                }
                if udp_timer.should_update(SAMPLE_INTERVAL_UDP_MS) {
                    Self::process_net_stats(&mut udp_stats);
                }
            }
            if config.report_os_memory_stats && mem_timer.should_update(SAMPLE_INTERVAL_MEM_MS) {
                Self::report_mem_stats();
            }
            if config.report_os_cpu_stats && cpu_timer.should_update(SAMPLE_INTERVAL_CPU_MS) {
                Self::report_cpu_stats();
            }
            if config.report_os_disk_stats && disk_timer.should_update(SAMPLE_INTERVAL_DISK_MS) {
                Self::process_disk_stats(&mut disk_stats);
            }
            sleep(SLEEP_INTERVAL);
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_udp_stats() {
        const MOCK_SNMP: &[u8] =
b"Ip: Forwarding DefaultTTL InReceives InHdrErrors InAddrErrors ForwDatagrams InUnknownProtos InDiscards InDelivers OutRequests OutDiscards OutNoRoutes ReasmTimeout ReasmReqds ReasmOKs ReasmFails FragOKs FragFails FragCreates
Ip: 1 64 357 0 2 0 0 0 355 315 0 6 0 0 0 0 0 0 0
Icmp: InMsgs InErrors InCsumErrors InDestUnreachs InTimeExcds InParmProbs InSrcQuenchs InRedirects InEchos InEchoReps InTimestamps InTimestampReps InAddrMasks InAddrMaskReps OutMsgs OutErrors OutDestUnreachs OutTimeExcds OutParmProbs OutSrcQuenchs OutRedirects OutEchos OutEchoReps OutTimestamps OutTimestampReps OutAddrMasks OutAddrMaskReps
Icmp: 3 0 0 3 0 0 0 0 0 0 0 0 0 0 7 0 7 0 0 0 0 0 0 0 0 0 0
IcmpMsg: InType3 OutType3
IcmpMsg: 3 7
Tcp: RtoAlgorithm RtoMin RtoMax MaxConn ActiveOpens PassiveOpens AttemptFails EstabResets CurrEstab InSegs OutSegs RetransSegs InErrs OutRsts InCsumErrors
Tcp: 1 200 120000 -1 29 1 0 0 5 318 279 0 0 4 0
Udp: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors InCsumErrors IgnoredMulti
Udp: 27 7 0 30 0 0 0 0
UdpLite: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors InCsumErrors IgnoredMulti
UdpLite: 0 0 0 0 0 0 0 0" as &[u8];
        const UNEXPECTED_DATA: &[u8] = b"unexpected data" as &[u8];

        let mut mock_snmp = MOCK_SNMP;
        let stats = parse_udp_stats(&mut mock_snmp).unwrap();
        assert_eq!(stats.out_datagrams, 30);
        assert_eq!(stats.no_ports, 7);

        mock_snmp = UNEXPECTED_DATA;
        let stats = parse_udp_stats(&mut mock_snmp);
        assert!(stats.is_err());
    }

    #[test]
    fn test_parse_net_dev_stats() {
        const MOCK_DEV: &[u8] =
b"Inter-|   Receive                                                |  Transmit
face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
lo: 50     1    0    0    0     0          0         0 100 2    1    0    0     0       0          0
eno1: 100     1    0    0    0     0          0         0 200 3    2    0    0     0       0          0
ens4: 400     4    0    1    0     0          0         0 250 5    0    0    0     0       0          0" as &[u8];
        const UNEXPECTED_DATA: &[u8] = b"un
expected
data" as &[u8];

        let mut mock_dev = MOCK_DEV;
        let stats = parse_net_dev_stats(&mut mock_dev).unwrap();
        assert_eq!(stats.rx_bytes, 500);
        assert_eq!(stats.rx_packets, 5);
        assert_eq!(stats.rx_errs, 0);
        assert_eq!(stats.rx_drops, 1);
        assert_eq!(stats.tx_bytes, 450);
        assert_eq!(stats.tx_packets, 8);
        assert_eq!(stats.tx_errs, 2);
        assert_eq!(stats.tx_drops, 0);

        let mut mock_dev = UNEXPECTED_DATA;
        let stats = parse_net_dev_stats(&mut mock_dev);
        assert!(stats.is_err());
    }

    #[test]
    fn test_parse_disk_stats() {
        const MOCK_DISK_11: &[u8] =
b" 2095701   479815 122620302  1904439 43496218 26953623 3935324729 283313376        0  6101780 285220738" as &[u8];
        // Matches kernel 4.18+ format
        const MOCK_DISK_15: &[u8] =
b" 2095701   479815 122620302  1904439 43496218 26953623 3935324729 283313376        0  6101780 285220738        0        0        0        0" as &[u8];
        // Matches kernel 5.5+ format
        const MOCK_DISK_17: &[u8] =
b" 2095701   479815 122620302  1904439 43496218 26953623 3935324729 283313376        0  6101780 285220738        0        0        0        0    70715     2922" as &[u8];
        const UNEXPECTED_DATA_1: &[u8] =
b" 2095701   479815 122620302  1904439 43496218 26953623 3935324729 283313376        0  6101780 285220738        0        0        0        0    70715" as &[u8];

        const UNEXPECTED_DATA_2: &[u8] = b"un
ex
pec
ted
data" as &[u8];

        let mut mock_disk = MOCK_DISK_11;
        let stats = parse_disk_stats(&mut mock_disk).unwrap();
        assert_eq!(stats.reads_completed, 2095701);
        assert_eq!(stats.time_io_weighted_ms, 285220738);

        let mut mock_disk = MOCK_DISK_15;
        let stats = parse_disk_stats(&mut mock_disk).unwrap();
        assert_eq!(stats.reads_completed, 2095701);
        assert_eq!(stats.time_discarding, 0);

        let mut mock_disk = MOCK_DISK_17;
        let stats = parse_disk_stats(&mut mock_disk).unwrap();
        assert_eq!(stats.reads_completed, 2095701);
        assert_eq!(stats.time_flushing, 2922);

        let mut mock_disk = UNEXPECTED_DATA_1;
        let stats = parse_disk_stats(&mut mock_disk);
        assert!(stats.is_err());

        let mut mock_disk = UNEXPECTED_DATA_2;
        let stats = parse_disk_stats(&mut mock_disk);
        assert!(stats.is_err());
    }

    #[test]
    fn test_calc_percent() {
        assert!(SystemMonitorService::calc_percent(99, 100) < 100.0);
        let one_tb_as_kb = (1u64 << 40) >> 10;
        assert!(SystemMonitorService::calc_percent(one_tb_as_kb - 1, one_tb_as_kb) < 100.0);
    }
}
