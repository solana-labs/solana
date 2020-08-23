use std::borrow::Borrow;
use std::cmp::Eq;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::AddAssign;
use std::time::{Duration, Instant};

/// A fixed lookback window maintains some statistics over the half-open time
/// interval:
///    [start  <- duration ->  end)
/// When time passes the end of the window, a new window is instantiated.
/// Compared to "rolling" windows, it has the advantage of computational and
/// memory efficiency. However, at the beginning of each window, it starts from
/// scratch and has zero data. To mitigate this, this implementation uses two
/// windows simultaneously, one starting half the interval after the other.
/// When looking up the values, it uses the interval which starts earlier.
pub struct FixedWindow<K, V> {
    /// Length of lookback window.
    duration: Duration,
    /// Beginning of each lookback window.
    /// One should start half the duration after the other.
    start: [Instant; 2],
    table: [HashMap<K, V>; 2],
}

impl<K, V> FixedWindow<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new(now: Instant, duration: Duration) -> Self {
        FixedWindow {
            duration,
            start: [now, now + duration / 2],
            table: [HashMap::new(), HashMap::new()],
        }
    }

    /// Looks up the key from the lookback window.
    /// Returns None if the key does not exist or if both windows are expired.
    pub fn get<Q>(&self, now: Instant, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        // If both windows are expired, i.e.
        //   self.start[i] + self.duration <= now
        // then return None.
        // If exactly one of the two windows is expired, then look up from the
        // other window. Otherwise, pick the window which starts earlier.
        if self.start[0] + self.duration <= now {
            if self.start[1] + self.duration <= now {
                None
            } else {
                self.table[1].get(key)
            }
        } else if self.start[1] + self.duration <= now || self.start[0] < self.start[1] {
            self.table[0].get(key)
        } else {
            self.table[1].get(key)
        }
    }

    /// Records a new value for the lookback window.
    pub fn add<I>(&mut self, now: Instant, key: K, item: I)
    where
        V: From<I> + AddAssign<I>,
        I: Clone,
    {
        for i in 0..2 {
            // If the window is expired, discard the values and restart.
            if self.start[i] + self.duration <= now {
                self.start[i] = Instant::max(now, self.start[1 - i] + self.duration / 2);
                self.table[i].clear();
            }
            if self.start[i] <= now {
                self.table[i]
                    .entry(key.clone())
                    .and_modify(|e| *e += item.clone())
                    .or_insert_with(|| item.clone().into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_window() {
        let now = Instant::now();
        let mut win = FixedWindow::new(now, Duration::from_secs(4));
        let key = String::from("abc");

        assert_eq!(win.get(now, &key), None);

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 1);
        assert_eq!(win.get(now, &key), Some(&1));

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 2);
        assert_eq!(win.get(now, &key), Some(&3)); // 1 + 2

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 4);
        assert_eq!(win.get(now, &key), Some(&6)); // 2 + 4

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 7);
        assert_eq!(win.get(now, &key), Some(&11)); // 4 + 7

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 12);
        assert_eq!(win.get(now, &key), Some(&19)); // 7 + 12

        let now = now + Duration::from_secs(2);
        win.add(now, key.clone(), 20);
        assert_eq!(win.get(now, &key), Some(&32)); // 12 + 20

        let now = now + Duration::from_secs(1);
        assert_eq!(win.get(now, &key), Some(&32));

        let now = now + Duration::from_secs(1);
        // 4 seconds after '12' was added, it falls off the window.
        assert_eq!(win.get(now, &key), Some(&20));

        let now = now + Duration::from_secs(1);
        assert_eq!(win.get(now, &key), Some(&20));

        let now = now + Duration::from_secs(1);
        // 4 seconds after '20' was added, it falls off the window.
        assert_eq!(win.get(now, &key), None);
    }

    #[derive(Debug, PartialEq)]
    struct Stat {
        hist: Vec<i32>,
    }

    impl From<i32> for Stat {
        fn from(x: i32) -> Self {
            Stat { hist: vec![x] }
        }
    }

    impl AddAssign<i32> for Stat {
        fn add_assign(&mut self, rhs: i32) {
            self.hist.push(rhs);
        }
    }

    #[test]
    fn test_fixed_window_hist() {
        let now = Instant::now();
        let mut win = FixedWindow::new(now, Duration::from_secs(3));
        let key = String::from("abc");

        assert_eq!(win.get(now, &key), None);

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 1);
        assert_eq!(win.get(now, &key), Some(&Stat { hist: vec![1] }));

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 2);
        assert_eq!(win.get(now, &key), Some(&Stat { hist: vec![1, 2] }));

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 3);
        assert_eq!(win.get(now, &key), Some(&Stat { hist: vec![2, 3] }));

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 4);
        assert_eq!(
            win.get(now, &key),
            Some(&Stat {
                hist: vec![2, 3, 4]
            })
        );

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 5);
        assert_eq!(
            win.get(now, &key),
            Some(&Stat {
                hist: vec![3, 4, 5]
            })
        );

        let now = now + Duration::from_secs(1);
        win.add(now, key.clone(), 6);
        assert_eq!(win.get(now, &key), Some(&Stat { hist: vec![5, 6] }));

        let now = now + Duration::from_secs(1);
        assert_eq!(win.get(now, &key), Some(&Stat { hist: vec![5, 6] }));

        let now = now + Duration::from_secs(1);
        assert_eq!(win.get(now, &key), None);
    }
}
