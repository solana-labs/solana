#[cfg(target_os = "linux")]
use std::{fs::File, io::BufReader, process::Command};
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
const SAMPLE_INTERVAL_MEM_MS: u64 = MS_PER_S;
const SAMPLE_INTERVAL_CPU_MS: u64 = MS_PER_S;
const SAMPLE_INTERVAL_IO_MS: u64 = MS_PER_S;
const SLEEP_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(target_os = "linux")]
const PROC_NET_SNMP_PATH: &str = "/proc/net/snmp";
#[cfg(target_os = "linux")]
const PROC_NET_DEV_PATH: &str = "/proc/net/dev";

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

#[derive(Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
// The first collection provides statistics concerning the time since the system
// was booted. Each subsequent one covers the time since the previous one.
struct IoStats {
    // read operations per second across all disks
    read_iops: f64,
    // read MB per second across all disks
    read_mbps: f64,
    // read requests merged per second and queued across all devices
    read_req_merged_per_second: f64,
    // percentage of read requests merged (averaged across devices)
    read_req_merged_percent: f64,
    // average time in milliseconds for read requests to be served
    read_avg_await_ms: f64,
    // average number of sectors per read request
    read_avg_req_sectors: f64,
    // write operations per second across all disks
    write_iops: f64,
    // write MB per second across all disks
    write_mbps: f64,
    // write requests merged per second and queued across all devices
    write_req_merged_per_second: f64,
    // percentage of write requests merged (averaged across devices)
    write_req_merged_percent: f64,
    // average time in milliseconds for write requests to be served
    write_avg_await_ms: f64,
    // average number of sectors per write request
    write_avg_req_sectors: f64,
    // discard operations per second across all disks
    discard_iops: f64,
    // discard MB per second across all disks
    discard_mbps: f64,
    // discards requests merged per second and queued across all devices
    discard_req_merged_per_second: f64,
    // percentage of discard requests merged (averaged across devices)
    discard_req_merged_percent: f64,
    // average time in milliseconds for discard requests to be served
    discard_avg_await_ms: f64,
    // average number of sectors per discard request
    discard_avg_req_sectors: f64,
    // average number of commands in the queue (averaged across devices)
    avg_queue_length: f64,
    // percentage of time IO requests were issued to device (averaged across devices)
    utilization_percent_avg: f64,
    // percentage of time IO requests were issued to device (max of any single device)
    utilization_percent_max: f64,
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
fn read_io_stats() -> Result<IoStats, String> {
    let output = Command::new("iostat")
        .arg("-d")
        .arg("-m")
        .arg("-x")
        .output()
        .map_err(|e| e.to_string())?;
    let mut reader_io = &output.stdout as &[u8];
    parse_io_stats(&mut reader_io)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_io_stats(reader_io: &mut impl BufRead) -> Result<IoStats, String> {
    let mut stats = IoStats::default();
    let mut num_devices = 0 as f64;
    for (line_number, line) in reader_io.lines().enumerate() {
        if line_number < 3 {
            // Skip first three lines with header information.
            continue;
        }

        let line = line.map_err(|e| e.to_string())?;
        let values: Vec<_> = line.split_ascii_whitespace().collect();

        if values.is_empty() {
            // Filter out empty lines.
            continue;
        } else if values.len() != 21 {
            return Err("parse error, expected exactly 21 io stat elements".to_string());
        } else if values[0].starts_with("loop") {
            // Filter out the loopback io devices as we are only concerned with
            // physical disks.
            continue;
        }

        num_devices += 1.0;
        stats.read_iops += values[1].parse::<f64>().map_err(|e| e.to_string())?;
        stats.read_mbps += values[2].parse::<f64>().map_err(|e| e.to_string())?;
        stats.read_req_merged_per_second += values[3].parse::<f64>().map_err(|e| e.to_string())?;
        stats.read_req_merged_percent += values[4].parse::<f64>().map_err(|e| e.to_string())?;
        stats.read_avg_await_ms += values[5].parse::<f64>().map_err(|e| e.to_string())?;
        stats.read_avg_req_sectors += values[6].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_iops += values[7].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_mbps += values[8].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_req_merged_per_second += values[9].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_req_merged_percent += values[10].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_avg_await_ms += values[11].parse::<f64>().map_err(|e| e.to_string())?;
        stats.write_avg_req_sectors += values[12].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_iops += values[13].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_mbps += values[14].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_req_merged_per_second +=
            values[15].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_req_merged_percent += values[16].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_avg_await_ms += values[17].parse::<f64>().map_err(|e| e.to_string())?;
        stats.discard_avg_req_sectors += values[18].parse::<f64>().map_err(|e| e.to_string())?;
        stats.avg_queue_length += values[19].parse::<f64>().map_err(|e| e.to_string())?;
        stats.utilization_percent_avg += values[20].parse::<f64>().map_err(|e| e.to_string())?;
        stats.utilization_percent_max = stats
            .utilization_percent_max
            .max(values[20].parse::<f64>().map_err(|e| e.to_string())?);
    }

    if num_devices > 1.0 {
        stats.read_req_merged_percent /= num_devices;
        stats.read_avg_await_ms /= num_devices;
        stats.read_avg_req_sectors /= num_devices;
        stats.write_req_merged_percent /= num_devices;
        stats.write_avg_await_ms /= num_devices;
        stats.write_avg_req_sectors /= num_devices;
        stats.discard_req_merged_percent /= num_devices;
        stats.discard_avg_await_ms /= num_devices;
        stats.discard_avg_req_sectors /= num_devices;
        stats.avg_queue_length /= num_devices;
        stats.utilization_percent_avg /= num_devices;
    }

    Ok(stats)
}

impl SystemMonitorService {
    pub fn new(
        exit: Arc<AtomicBool>,
        report_os_memory_stats: bool,
        report_os_network_stats: bool,
        report_os_cpu_stats: bool,
        report_os_io_stats: bool,
    ) -> Self {
        info!("Starting SystemMonitorService");
        let thread_hdl = Builder::new()
            .name("system-monitor".to_string())
            .spawn(move || {
                Self::run(
                    exit,
                    report_os_memory_stats,
                    report_os_network_stats,
                    report_os_cpu_stats,
                    report_os_io_stats,
                );
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
    }

    #[cfg(target_os = "linux")]
    fn report_io_stats() {
        match read_io_stats() {
            Ok(stats) => {
                datapoint_info!(
                    "io-stats",
                    ("read_iops", stats.read_iops, f64),
                    ("read_mbps", stats.read_mbps, f64),
                    (
                        "read_req_merged_per_second",
                        stats.read_req_merged_per_second,
                        f64
                    ),
                    (
                        "read_req_merged_percent",
                        stats.read_req_merged_percent,
                        f64
                    ),
                    ("read_avg_await_ms", stats.read_avg_await_ms, f64),
                    ("read_avg_req_sectors", stats.read_avg_req_sectors, f64),
                    ("write_iops", stats.write_iops, f64),
                    ("write_mbps", stats.write_mbps, f64),
                    (
                        "write_req_merged_per_second",
                        stats.write_req_merged_per_second,
                        f64
                    ),
                    (
                        "write_req_merged_percent",
                        stats.write_req_merged_percent,
                        f64
                    ),
                    ("write_avg_await_ms", stats.write_avg_await_ms, f64),
                    ("write_avg_req_sectors", stats.write_avg_req_sectors, f64),
                    ("discard_iops", stats.discard_iops, f64),
                    ("discard_mbps", stats.discard_mbps, f64),
                    (
                        "discard_req_merged_per_second",
                        stats.discard_req_merged_per_second,
                        f64
                    ),
                    (
                        "discard_req_merged_percent",
                        stats.discard_req_merged_percent,
                        f64
                    ),
                    ("discard_avg_await_ms", stats.discard_avg_await_ms, f64),
                    (
                        "discard_avg_req_sectors",
                        stats.discard_avg_req_sectors,
                        f64
                    ),
                    ("avg_queue_length", stats.avg_queue_length, f64),
                    (
                        "utilization_percent_avg",
                        stats.utilization_percent_avg,
                        f64
                    ),
                    (
                        "utilization_percent_max",
                        stats.utilization_percent_max,
                        f64
                    ),
                )
            }
            Err(e) => warn!("read_io_stats: {}", e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn report_io_stats() {}

    pub fn run(
        exit: Arc<AtomicBool>,
        report_os_memory_stats: bool,
        report_os_network_stats: bool,
        report_os_cpu_stats: bool,
        report_os_io_stats: bool,
    ) {
        let mut udp_stats = None;
        let network_limits_timer = AtomicInterval::default();
        let udp_timer = AtomicInterval::default();
        let mem_timer = AtomicInterval::default();
        let cpu_timer = AtomicInterval::default();
        let io_timer = AtomicInterval::default();

        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            if report_os_network_stats {
                if network_limits_timer.should_update(SAMPLE_INTERVAL_OS_NETWORK_LIMITS_MS) {
                    Self::check_os_network_limits();
                }
                if udp_timer.should_update(SAMPLE_INTERVAL_UDP_MS) {
                    Self::process_net_stats(&mut udp_stats);
                }
            }
            if report_os_memory_stats && mem_timer.should_update(SAMPLE_INTERVAL_MEM_MS) {
                Self::report_mem_stats();
            }
            if report_os_cpu_stats && cpu_timer.should_update(SAMPLE_INTERVAL_CPU_MS) {
                Self::report_cpu_stats();
            }
            if report_os_io_stats && io_timer.should_update(SAMPLE_INTERVAL_IO_MS) {
                Self::report_io_stats();
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
    fn test_parse_io_stats() {
        const MOCK_IO: &[u8] =
b"Linux 5.15.0-1013-gcp (dev-1) 	07/26/22 	_x86_64_	(64 CPU)

Device            r/s     rMB/s   rrqm/s  %rrqm r_await rareq-sz     w/s     wMB/s   wrqm/s  %wrqm w_await wareq-sz     d/s     dMB/s   drqm/s  %drqm d_await dareq-sz  aqu-sz  %util
loop0            0.00      0.00     0.00   0.00    0.25    13.45    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop1            0.00      0.00     0.00   0.00    0.48     7.46    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop10           0.00      0.00     0.00   0.00    0.02     6.23    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop2            0.00      0.00     0.00   0.00    0.19    13.50    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop3            0.00      0.00     0.00   0.00    0.50     8.57    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop4            0.00      0.00     0.00   0.00    0.56    13.05    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop5            0.00      0.00     0.00   0.00    2.28    18.25    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop6            0.00      0.00     0.00   0.00    0.69    16.18    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop7            0.00      0.00     0.00   0.00    0.45    36.83    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop8            0.00      0.00     0.00   0.00    1.29     8.39    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
loop9            0.00      0.00     0.00   0.00    0.75    19.15    0.00      0.00     0.00   0.00    0.00     0.00    0.00      0.00     0.00   0.00    0.00     0.00    0.00   0.00
sda             69.50      1.04     0.53   0.75    0.73    15.24  606.05     32.35   353.50  36.84    5.96    54.67    0.93      1.73     0.00   0.00    0.34  1900.17    3.67  15.00
sdb             71.50      1.04     0.53   0.75    0.73    15.24  606.05     32.35   353.50  36.84    5.96    54.67    0.93      1.73     0.00   0.00    0.34  1900.17    3.67  95.00" as &[u8];
        const UNEXPECTED_DATA: &[u8] = b"un
ex
pec
ted
data" as &[u8];

        let mut mock_io = MOCK_IO;
        let stats = parse_io_stats(&mut mock_io).unwrap();
        assert_eq!(stats.read_iops, 141.00);
        assert_eq!(stats.utilization_percent_avg, 55.00);
        assert_eq!(stats.utilization_percent_max, 95.00);

        let mut mock_io = UNEXPECTED_DATA;
        let stats = parse_io_stats(&mut mock_io);
        assert!(stats.is_err());
    }

    #[test]
    fn test_bw_temp() {
        solana_logger::setup();
        SystemMonitorService::report_io_stats();
    }

    #[test]
    fn test_calc_percent() {
        assert!(SystemMonitorService::calc_percent(99, 100) < 100.0);
        let one_tb_as_kb = (1u64 << 40) >> 10;
        assert!(SystemMonitorService::calc_percent(one_tb_as_kb - 1, one_tb_as_kb) < 100.0);
    }
}
