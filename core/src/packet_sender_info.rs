/// struct PacketSenderInfo contains sender information for pakcets in baning_stage's
/// unprocessed_buffer. It provides metrics for stake weight block packing prioritization.
///
use std::{collections::HashMap, net::IpAddr};

#[derive(Debug, Default)]
pub struct SenderDetailInfo {
    pub stake: u64,
    pub packet_count: u64,
}

#[derive(Debug, Default)]
pub struct PacketSenderInfo {
    // Vec of IP of packets in banking_stage buffer, one-to-one mapped, even after shuffling.
    pub packet_senders_ip: Vec<IpAddr>,
    // HashMap keyed by IP, with sender's detail info as Value
    pub senders_detail: HashMap<IpAddr, SenderDetailInfo>,
    // implied info are:
    // 1. packet_senders_ip.len() is the number of buffered packets subjected to this round of
    //    processing;
    // 2. sender_detail.values.map(|info| info.stake).sum() is the total stakes presented in buffer
}

impl PacketSenderInfo {
    pub fn report(&self, id: u32) {
        let tot_buffered_packet_count = self.packet_senders_ip.len();
        let tot_buffered_stakes: u64 = self.senders_detail.values().map(|info| info.stake).sum();

        for (ip, sender_info) in self.senders_detail.iter() {
            let stake_pct = match tot_buffered_stakes == 0 {
                true => 0.0,
                false => sender_info.stake as f64 / tot_buffered_stakes as f64,
            };
            let transaction_pct = match tot_buffered_packet_count == 0 {
                true => 0.0,
                false => sender_info.packet_count as f64 / tot_buffered_packet_count as f64,
            };

            datapoint_info!(
                "banking_stage-packing_sender_info",
                ("id", id as i64, i64),
                ("ip", ip.to_string(), String),
                ("stake", sender_info.stake as i64, i64),
                ("tot_buffered_stakes", tot_buffered_stakes as i64, i64),
                ("stake_pct", stake_pct, f64),
                ("packet_count", sender_info.packet_count as i64, i64),
                (
                    "tot_buffered_packet_count",
                    tot_buffered_packet_count as i64,
                    i64
                ),
                ("packet_pct", transaction_pct, f64),
            );
        }
    }
}
