use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct ActiveStats {
    clean: AtomicUsize,
    shrink: AtomicUsize,
    hash: AtomicUsize,
    flush: AtomicUsize,
}

#[derive(Debug, Copy, Clone)]
pub enum ActiveStatItems {
    Clean,
    Shrink,
    Hash,
    Flush,
}

pub struct ActiveState<'a> {
    stats: &'a ActiveStats,
    item: ActiveStatItems,
}

impl<'a> Drop for ActiveState<'a> {
    fn drop(&mut self) {
        self.stats.log(self.item, false);
    }
}

impl ActiveStats {
    #[must_use]
    pub fn get_state(&self, stat: ActiveStatItems) -> ActiveState<'_> {
        self.log(stat, true);
        ActiveState {
            stats: self,
            item: stat,
        }
    }
    pub fn log(&self, item: ActiveStatItems, increment: bool) {
        let stat = match item {
            ActiveStatItems::Clean => &self.clean,
            ActiveStatItems::Shrink => &self.shrink,
            ActiveStatItems::Hash => &self.hash,
            ActiveStatItems::Flush => &self.flush,
        };
        if increment {
            stat.fetch_add(1, Ordering::Relaxed);
        } else {
            stat.fetch_sub(1, Ordering::Relaxed);
        }
        let value = stat.load(Ordering::Relaxed);
        match item {
            ActiveStatItems::Clean => datapoint_info!("accounts_db_active", ("clean", value, i64)),
            ActiveStatItems::Shrink => {
                datapoint_info!("accounts_db_active", ("shrink", value, i64))
            }
            ActiveStatItems::Hash => datapoint_info!("accounts_db_active", ("hash", value, i64)),
            ActiveStatItems::Flush => datapoint_info!("accounts_db_active", ("flush", value, i64)),
        };
    }
}
