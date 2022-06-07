#[cfg(target_os = "linux")]
use std::{fs::File, io::BufReader, path::Path};
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
const SLEEP_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(target_os = "linux")]
const PROC_NET_SNMP_PATH: &str = "/proc/net/snmp";

pub struct SystemMonitorService {
    thread_hdl: JoinHandle<()>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct UdpStats {
    in_datagrams: usize,
    no_ports: usize,
    in_errors: usize,
    out_datagrams: usize,
    rcvbuf_errors: usize,
    sndbuf_errors: usize,
    in_csum_errors: usize,
    ignored_multi: usize,
}

struct CpuInfo {
    cpu_num: u32,
    cpu_freq_mhz: u64,
    load_avg: LoadAvg,
    num_threads: u64,
}

impl UdpStats {
    fn from_map(udp_stats: &HashMap<String, usize>) -> Self {
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
fn read_udp_stats(file_path: impl AsRef<Path>) -> Result<UdpStats, String> {
    let file = File::open(file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    parse_udp_stats(&mut reader)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_udp_stats(reader: &mut impl BufRead) -> Result<UdpStats, String> {
    let mut udp_lines = Vec::default();
    for line in reader.lines() {
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
    let udp_stats: HashMap<String, usize> = pairs[1..]
        .iter()
        .map(|(label, val)| (label.to_string(), val.parse::<usize>().unwrap()))
        .collect();

    let stats = UdpStats::from_map(&udp_stats);

    Ok(stats)
}

#[cfg(target_os = "linux")]
pub fn verify_udp_stats_access() -> Result<(), String> {
    read_udp_stats(PROC_NET_SNMP_PATH)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn verify_udp_stats_access() -> Result<(), String> {
    Ok(())
}

impl SystemMonitorService {
    pub fn new(
        exit: Arc<AtomicBool>,
        report_os_memory_stats: bool,
        report_os_network_stats: bool,
        report_os_cpu_stats: bool,
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
    fn process_udp_stats(udp_stats: &mut Option<UdpStats>) {
        match read_udp_stats(PROC_NET_SNMP_PATH) {
            Ok(new_stats) => {
                if let Some(old_stats) = udp_stats {
                    Self::report_udp_stats(old_stats, &new_stats);
                }
                *udp_stats = Some(new_stats);
            }
            Err(e) => warn!("read_udp_stats: {}", e),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn process_udp_stats(_udp_stats: &mut Option<UdpStats>) {}

    #[cfg(target_os = "linux")]
    fn report_udp_stats(old_stats: &UdpStats, new_stats: &UdpStats) {
        datapoint_info!(
            "net-stats-validator",
            (
                "in_datagrams_delta",
                new_stats.in_datagrams - old_stats.in_datagrams,
                i64
            ),
            (
                "no_ports_delta",
                new_stats.no_ports - old_stats.no_ports,
                i64
            ),
            (
                "in_errors_delta",
                new_stats.in_errors - old_stats.in_errors,
                i64
            ),
            (
                "out_datagrams_delta",
                new_stats.out_datagrams - old_stats.out_datagrams,
                i64
            ),
            (
                "rcvbuf_errors_delta",
                new_stats.rcvbuf_errors - old_stats.rcvbuf_errors,
                i64
            ),
            (
                "sndbuf_errors_delta",
                new_stats.sndbuf_errors - old_stats.sndbuf_errors,
                i64
            ),
            (
                "in_csum_errors_delta",
                new_stats.in_csum_errors - old_stats.in_csum_errors,
                i64
            ),
            (
                "ignored_multi_delta",
                new_stats.ignored_multi - old_stats.ignored_multi,
                i64
            ),
            ("in_errors", new_stats.in_errors, i64),
            ("rcvbuf_errors", new_stats.rcvbuf_errors, i64),
            ("sndbuf_errors", new_stats.sndbuf_errors, i64),
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

    pub fn run(
        exit: Arc<AtomicBool>,
        report_os_memory_stats: bool,
        report_os_network_stats: bool,
        report_os_cpu_stats: bool,
    ) {
        let mut udp_stats = None;
        let network_limits_timer = AtomicInterval::default();
        let udp_timer = AtomicInterval::default();
        let mem_timer = AtomicInterval::default();
        let cpu_timer = AtomicInterval::default();

        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            if report_os_network_stats {
                if network_limits_timer.should_update(SAMPLE_INTERVAL_OS_NETWORK_LIMITS_MS) {
                    Self::check_os_network_limits();
                }
                if udp_timer.should_update(SAMPLE_INTERVAL_UDP_MS) {
                    Self::process_udp_stats(&mut udp_stats);
                }
            }
            if report_os_memory_stats && mem_timer.should_update(SAMPLE_INTERVAL_MEM_MS) {
                Self::report_mem_stats();
            }
            if report_os_cpu_stats && cpu_timer.should_update(SAMPLE_INTERVAL_CPU_MS) {
                Self::report_cpu_stats();
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
        let mut mock_snmp =
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
        let stats = parse_udp_stats(&mut mock_snmp).unwrap();
        assert_eq!(stats.out_datagrams, 30);
        assert_eq!(stats.no_ports, 7);

        let mut mock_snmp = b"unexpected data" as &[u8];
        let stats = parse_udp_stats(&mut mock_snmp);
        assert!(stats.is_err());
    }

    #[test]
    fn test_calc_percent() {
        assert!(SystemMonitorService::calc_percent(99, 100) < 100.0);
        let one_tb_as_kb = (1u64 << 40) >> 10;
        assert!(SystemMonitorService::calc_percent(one_tb_as_kb - 1, one_tb_as_kb) < 100.0);
    }
}
