//! A library for creating vesting schedules

use chrono::prelude::*;

/// Return the date that is 'n' months from 'start'.
fn get_month(start: Date<Utc>, n: u32) -> Date<Utc> {
    let year = start.year() + (start.month0() + n) as i32 / 12;
    let month0 = (start.month0() + n) % 12;

    // For those that started on the 31st, pay out on the latest day of the month.
    let mut dt = None;
    let mut days_back = 0;
    while dt.is_none() {
        dt = Utc
            .ymd_opt(year, month0 + 1, start.day() - days_back)
            .single();
        days_back += 1;
    }
    dt.unwrap()
}

/// Integer division that also returns the remainder.
fn div(dividend: u64, divisor: u64) -> (u64, u64) {
    (dividend / divisor, dividend % divisor)
}

/// Return a list of contract messages and a list of vesting-date/lamports pairs.
pub fn create_vesting_schedule(start_date: Date<Utc>, mut lamports: u64) -> Vec<(Date<Utc>, u64)> {
    let mut schedule = vec![];

    // 1/3 vest after one year from start date.
    let (mut stipend, remainder) = div(lamports, 3);
    stipend += remainder;

    let dt = get_month(start_date, 12);
    schedule.push((dt, stipend));

    lamports -= stipend;

    // Remaining 66% vest monthly after one year.
    let payments = 24u32;
    let (stipend, remainder) = div(lamports, u64::from(payments));
    for n in 0..payments {
        let mut stipend = stipend;
        if u64::from(n) < remainder {
            stipend += 1;
        }
        let dt = get_month(start_date, n + 13);
        schedule.push((dt, stipend));
        lamports -= stipend;
    }
    assert_eq!(lamports, 0);

    schedule
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_month() {
        let start = Utc.ymd(2018, 1, 31);
        assert_eq!(get_month(start, 0), Utc.ymd(2018, 1, 31));
        assert_eq!(get_month(start, 1), Utc.ymd(2018, 2, 28));
        assert_eq!(get_month(start, 2), Utc.ymd(2018, 3, 31));
    }

    #[test]
    fn test_create_vesting_schedule() {
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 1), 36_000),
            vec![
                (Utc.ymd(2019, 1, 1), 12000),
                (Utc.ymd(2019, 2, 1), 1000),
                (Utc.ymd(2019, 3, 1), 1000),
                (Utc.ymd(2019, 4, 1), 1000),
                (Utc.ymd(2019, 5, 1), 1000),
                (Utc.ymd(2019, 6, 1), 1000),
                (Utc.ymd(2019, 7, 1), 1000),
                (Utc.ymd(2019, 8, 1), 1000),
                (Utc.ymd(2019, 9, 1), 1000),
                (Utc.ymd(2019, 10, 1), 1000),
                (Utc.ymd(2019, 11, 1), 1000),
                (Utc.ymd(2019, 12, 1), 1000),
                (Utc.ymd(2020, 1, 1), 1000),
                (Utc.ymd(2020, 2, 1), 1000),
                (Utc.ymd(2020, 3, 1), 1000),
                (Utc.ymd(2020, 4, 1), 1000),
                (Utc.ymd(2020, 5, 1), 1000),
                (Utc.ymd(2020, 6, 1), 1000),
                (Utc.ymd(2020, 7, 1), 1000),
                (Utc.ymd(2020, 8, 1), 1000),
                (Utc.ymd(2020, 9, 1), 1000),
                (Utc.ymd(2020, 10, 1), 1000),
                (Utc.ymd(2020, 11, 1), 1000),
                (Utc.ymd(2020, 12, 1), 1000),
                (Utc.ymd(2021, 1, 1), 1000),
            ]
        );

        // Ensure vesting date is sensible if start date was at the end of the month.
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 31), 36_000),
            vec![
                (Utc.ymd(2019, 1, 31), 12000),
                (Utc.ymd(2019, 2, 28), 1000),
                (Utc.ymd(2019, 3, 31), 1000),
                (Utc.ymd(2019, 4, 30), 1000),
                (Utc.ymd(2019, 5, 31), 1000),
                (Utc.ymd(2019, 6, 30), 1000),
                (Utc.ymd(2019, 7, 31), 1000),
                (Utc.ymd(2019, 8, 31), 1000),
                (Utc.ymd(2019, 9, 30), 1000),
                (Utc.ymd(2019, 10, 31), 1000),
                (Utc.ymd(2019, 11, 30), 1000),
                (Utc.ymd(2019, 12, 31), 1000),
                (Utc.ymd(2020, 1, 31), 1000),
                (Utc.ymd(2020, 2, 29), 1000), // Leap year
                (Utc.ymd(2020, 3, 31), 1000),
                (Utc.ymd(2020, 4, 30), 1000),
                (Utc.ymd(2020, 5, 31), 1000),
                (Utc.ymd(2020, 6, 30), 1000),
                (Utc.ymd(2020, 7, 31), 1000),
                (Utc.ymd(2020, 8, 31), 1000),
                (Utc.ymd(2020, 9, 30), 1000),
                (Utc.ymd(2020, 10, 31), 1000),
                (Utc.ymd(2020, 11, 30), 1000),
                (Utc.ymd(2020, 12, 31), 1000),
                (Utc.ymd(2021, 1, 31), 1000),
            ]
        );

        // Awkward numbers
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 1), 123_123),
            vec![
                (Utc.ymd(2019, 1, 1), 41041), // floor(123_123 / 3) + 123_123 % 3
                (Utc.ymd(2019, 2, 1), 3421),  // ceil(82_082 / 24)
                (Utc.ymd(2019, 3, 1), 3421),  // ceil(82_082 / 24)
                (Utc.ymd(2019, 4, 1), 3420),  // floor(82_082 / 24)
                (Utc.ymd(2019, 5, 1), 3420),
                (Utc.ymd(2019, 6, 1), 3420),
                (Utc.ymd(2019, 7, 1), 3420),
                (Utc.ymd(2019, 8, 1), 3420),
                (Utc.ymd(2019, 9, 1), 3420),
                (Utc.ymd(2019, 10, 1), 3420),
                (Utc.ymd(2019, 11, 1), 3420),
                (Utc.ymd(2019, 12, 1), 3420),
                (Utc.ymd(2020, 1, 1), 3420),
                (Utc.ymd(2020, 2, 1), 3420),
                (Utc.ymd(2020, 3, 1), 3420),
                (Utc.ymd(2020, 4, 1), 3420),
                (Utc.ymd(2020, 5, 1), 3420),
                (Utc.ymd(2020, 6, 1), 3420),
                (Utc.ymd(2020, 7, 1), 3420),
                (Utc.ymd(2020, 8, 1), 3420),
                (Utc.ymd(2020, 9, 1), 3420),
                (Utc.ymd(2020, 10, 1), 3420),
                (Utc.ymd(2020, 11, 1), 3420),
                (Utc.ymd(2020, 12, 1), 3420),
                (Utc.ymd(2021, 1, 1), 3420),
            ]
        );
    }
}
