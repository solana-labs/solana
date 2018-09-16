use std::cmp::max;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use sys_info::mem_info;

pub fn mk_channel<T>() -> (SyncSender<T>, Receiver<T>) {
    let info = mem_info().expect("mk_channel::mem_info");
    let channel_size = info.total / 65536;
    let channel_size = max(channel_size, 100);
    sync_channel(channel_size as usize)
}
