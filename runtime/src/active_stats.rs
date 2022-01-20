use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct ActiveStats {
    clean: AtomicUsize,
    shrink: AtomicUsize,
    hash: AtomicUsize,
    flush: AtomicUsize,
}

#[derive(Debug, Copy, Clone)]
pub enum ActiveStatItem {
    Clean,
    Shrink,
    Hash,
    Flush,
}

/// sole purpose is to handle 'drop' so that stat is decremented when self is dropped
pub struct ActiveState<'a> {
    stats: &'a ActiveStats,
    item: ActiveStatItem,
}

impl<'a> Drop for ActiveState<'a> {
    fn drop(&mut self) {
        self.stats.log(self.item, false);
    }
}

impl ActiveStats {
    #[must_use]
    /// create a stack object to set the state to increment stat initially and decrement on drop
    pub fn get_state(&self, stat: ActiveStatItem) -> ActiveState<'_> {
        self.log(stat, true);
        ActiveState {
            stats: self,
            item: stat,
        }
    }
    /// update and log the change to the specified 'item'
    fn log(&self, item: ActiveStatItem, increment: bool) {
        let stat = match item {
            ActiveStatItem::Clean => &self.clean,
            ActiveStatItem::Shrink => &self.shrink,
            ActiveStatItem::Hash => &self.hash,
            ActiveStatItem::Flush => &self.flush,
        };
        if increment {
            stat.fetch_add(1, Ordering::Relaxed);
        } else {
            stat.fetch_sub(1, Ordering::Relaxed);
        }
        let value = stat.load(Ordering::Relaxed);
        match item {
            ActiveStatItem::Clean => datapoint_info!("accounts_db_active", ("clean", value, i64)),
            ActiveStatItem::Shrink => {
                datapoint_info!("accounts_db_active", ("shrink", value, i64))
            }
            ActiveStatItem::Hash => datapoint_info!("accounts_db_active", ("hash", value, i64)),
            ActiveStatItem::Flush => datapoint_info!("accounts_db_active", ("flush", value, i64)),
        };
    }
}
