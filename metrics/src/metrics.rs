//! The `metrics` module enables sending measurements to an `InfluxDB` instance

use influx_db_client as influxdb;
use influx_db_client::Point;
use lazy_static::lazy_static;
use log::*;
use solana_sdk::hash::hash;
use solana_sdk::timing;
use std::collections::HashMap;
use std::convert::Into;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Barrier, Mutex, Once, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use std::{cmp, env};
use sys_info::hostname;

#[macro_export]
macro_rules! datapoint {
    (@field $point:ident $name:expr, $string:expr, String) => {
            $point.add_field(
                    $name,
                    $crate::influxdb::Value::String($string));
    };
    (@field $point:ident $name:expr, $value:expr, i64) => {
            $point.add_field(
                    $name,
                    $crate::influxdb::Value::Integer($value as i64));
    };
    (@field $point:ident $name:expr, $value:expr, f64) => {
            $point.add_field(
                    $name,
                    $crate::influxdb::Value::Float($value as f64));
    };
    (@field $point:ident $name:expr, $value:expr, bool) => {
            $point.add_field(
                    $name,
                    $crate::influxdb::Value::Boolean($value as bool));
    };

    (@fields $point:ident) => {};
    (@fields $point:ident ($name:expr, $value:expr, $type:ident) , $($rest:tt)*) => {
        $crate::datapoint!(@field $point $name, $value, $type);
        $crate::datapoint!(@fields $point $($rest)*);
    };
    (@fields $point:ident ($name:expr, $value:expr, $type:ident)) => {
        $crate::datapoint!(@field $point $name, $value, $type);
    };

    (@point $name:expr, $($fields:tt)+) => {
        {
        let mut point = $crate::influxdb::Point::new(&$name);
        $crate::datapoint!(@fields point $($fields)+);
        point
        }
    };
    (@point $name:expr) => {
        $crate::influxdb::Point::new(&$name)
    };
    ($name:expr, $($fields:tt)+) => {
        if log_enabled!(log::Level::Debug) {
            $crate::submit($crate::datapoint!(@point $name, $($fields)+), log::Level::Debug);
        }
    };
}

#[macro_export]
macro_rules! datapoint_error {
    ($name:expr) => {
        if log_enabled!(log::Level::Error) {
            $crate::submit($crate::datapoint!(@point $name), log::Level::Error);
        }
    };
    ($name:expr, $($fields:tt)+) => {
        if log_enabled!(log::Level::Error) {
            $crate::submit($crate::datapoint!(@point $name, $($fields)+), log::Level::Error);
        }
    };
}

#[macro_export]
macro_rules! datapoint_warn {
    ($name:expr) => {
        if log_enabled!(log::Level::Warn) {
            $crate::submit($crate::datapoint!(@point $name), log::Level::Warn);
        }
    };
    ($name:expr, $($fields:tt)+) => {
        if log_enabled!(log::Level::Warn) {
            $crate::submit($crate::datapoint!(@point $name, $($fields)+), log::Level::Warn);
        }
    };
}

#[macro_export]
macro_rules! datapoint_info {
    ($name:expr) => {
        if log_enabled!(log::Level::Info) {
            $crate::submit($crate::datapoint!(@point $name), log::Level::Info);
        }
    };
    ($name:expr, $($fields:tt)+) => {
        if log_enabled!(log::Level::Info) {
            $crate::submit($crate::datapoint!(@point $name, $($fields)+), log::Level::Info);
        }
    };
}

#[macro_export]
macro_rules! datapoint_debug {
    ($name:expr) => {
        if log_enabled!(log::Level::Debug) {
            $crate::submit($crate::datapoint!(@point $name), log::Level::Debug);
        }
    };
    ($name:expr, $($fields:tt)+) => {
        if log_enabled!(log::Level::Debug) {
            $crate::submit($crate::datapoint!(@point $name, $($fields)+), log::Level::Debug);
        }
    };
}

type CounterMap = HashMap<(&'static str, u64), CounterPoint>;

#[derive(Clone, Debug)]
pub struct CounterPoint {
    pub name: &'static str,
    pub count: i64,
    pub timestamp: u64,
}

impl CounterPoint {
    #[cfg(test)]
    fn new(name: &'static str) -> Self {
        CounterPoint {
            name,
            count: 0,
            timestamp: 0,
        }
    }
}

impl Into<influxdb::Point> for CounterPoint {
    fn into(self) -> influxdb::Point {
        let mut point = influxdb::Point::new(self.name);
        point.add_field("count", influxdb::Value::Integer(self.count));
        point.add_timestamp(self.timestamp as i64);
        point
    }
}

#[derive(Debug)]
enum MetricsCommand {
    Flush(Arc<Barrier>),
    Submit(influxdb::Point, log::Level),
    SubmitCounter(CounterPoint, log::Level, u64),
}

struct MetricsAgent {
    sender: Sender<MetricsCommand>,
}

trait MetricsWriter {
    // Write the points and empty the vector.  Called on the internal
    // MetricsAgent worker thread.
    fn write(&self, points: Vec<influxdb::Point>);
}

struct InfluxDbMetricsWriter {
    client: Option<influxdb::Client>,
}

impl InfluxDbMetricsWriter {
    fn new() -> Self {
        Self {
            client: Self::build_client().ok(),
        }
    }

    fn build_client() -> Result<influxdb::Client, String> {
        let config = get_metrics_config().map_err(|err| {
            info!("metrics disabled: {}", err);
            err
        })?;

        info!(
            "metrics configuration: host={} db={} username={}",
            config.host, config.db, config.username
        );
        let mut client = influxdb::Client::new_with_option(config.host, config.db, None)
            .set_authentication(config.username, config.password);

        client.set_read_timeout(1 /*second*/);
        client.set_write_timeout(1 /*second*/);

        debug!("InfluxDB version: {:?}", client.get_version());
        Ok(client)
    }
}

impl MetricsWriter for InfluxDbMetricsWriter {
    fn write(&self, points: Vec<influxdb::Point>) {
        if let Some(ref client) = self.client {
            debug!("submitting {} points", points.len());
            if let Err(err) = client.write_points(
                influxdb::Points { point: points },
                Some(influxdb::Precision::Milliseconds),
                None,
            ) {
                debug!("InfluxDbMetricsWriter write error: {:?}", err);
            }
        }
    }
}

impl Default for MetricsAgent {
    fn default() -> Self {
        let max_points_per_sec = env::var("SOLANA_METRICS_MAX_POINTS_PER_SECOND")
            .map(|x| {
                x.parse()
                    .expect("Failed to parse SOLANA_METRICS_MAX_POINTS_PER_SECOND")
            })
            .unwrap_or(4000);

        Self::new(
            Arc::new(InfluxDbMetricsWriter::new()),
            Duration::from_secs(10),
            max_points_per_sec,
        )
    }
}

impl MetricsAgent {
    fn new(
        writer: Arc<dyn MetricsWriter + Send + Sync>,
        write_frequency_secs: Duration,
        max_points_per_sec: usize,
    ) -> Self {
        let (sender, receiver) = channel::<MetricsCommand>();
        thread::spawn(move || {
            Self::run(&receiver, &writer, write_frequency_secs, max_points_per_sec)
        });

        Self { sender }
    }

    fn write(
        host_id: &influxdb::Value,
        mut points: Vec<Point>,
        last_write_time: Instant,
        max_points: usize,
        writer: &Arc<dyn MetricsWriter + Send + Sync>,
        max_points_per_sec: usize,
    ) -> usize {
        if points.is_empty() {
            return 0;
        }

        let now = Instant::now();
        let num_points = points.len();
        debug!("run: attempting to write {} points", num_points);
        if num_points > max_points {
            warn!(
                "max submission rate of {} datapoints per second exceeded.  only the
                    first {} of {} points will be submitted",
                max_points_per_sec, max_points, num_points
            );
        }
        let points_written = cmp::min(num_points, max_points - 1);
        points.truncate(points_written);

        let extra = influxdb::Point::new("metrics")
            .add_timestamp(timing::timestamp() as i64)
            .add_tag("host_id", host_id.clone())
            .add_field(
                "points_written",
                influxdb::Value::Integer(points_written as i64),
            )
            .add_field("num_points", influxdb::Value::Integer(num_points as i64))
            .add_field(
                "points_lost",
                influxdb::Value::Integer((num_points - points_written) as i64),
            )
            .add_field(
                "secs_since_last_write",
                influxdb::Value::Integer(now.duration_since(last_write_time).as_secs() as i64),
            )
            .to_owned();

        for point in &mut points {
            // TODO: rework influx_db_client crate API to avoid this unnecessary cloning
            point.add_tag("host_id", host_id.clone());
        }

        writer.write(points);
        writer.write([extra].to_vec());

        points_written
    }

    fn run(
        receiver: &Receiver<MetricsCommand>,
        writer: &Arc<dyn MetricsWriter + Send + Sync>,
        write_frequency_secs: Duration,
        max_points_per_sec: usize,
    ) {
        trace!("run: enter");
        let mut last_write_time = Instant::now();
        let mut points_map = HashMap::<log::Level, (Instant, CounterMap, Vec<Point>)>::new();
        let max_points = write_frequency_secs.as_secs() as usize * max_points_per_sec;

        loop {
            match receiver.recv_timeout(write_frequency_secs / 2) {
                Ok(cmd) => match cmd {
                    MetricsCommand::Flush(barrier) => {
                        debug!("metrics_thread: flush");
                        points_map.drain().for_each(|(_, (_, counters, points))| {
                            let counter_points = counters.into_iter().map(|(_, v)| v.into());
                            let points: Vec<_> = points.into_iter().chain(counter_points).collect();
                            writer.write(points);
                            last_write_time = Instant::now();
                        });
                        barrier.wait();
                    }
                    MetricsCommand::Submit(point, level) => {
                        debug!("run: submit {:?}", point);
                        let (_, _, points) = points_map.entry(level).or_insert((
                            last_write_time,
                            HashMap::new(),
                            Vec::new(),
                        ));
                        points.push(point);
                    }
                    MetricsCommand::SubmitCounter(counter, level, bucket) => {
                        debug!("run: submit counter {:?}", counter);
                        let (_, counters, _) = points_map.entry(level).or_insert((
                            last_write_time,
                            HashMap::new(),
                            Vec::new(),
                        ));

                        let key = (counter.name, bucket);
                        if let Some(value) = counters.get_mut(&key) {
                            value.count += counter.count;
                        } else {
                            counters.insert(key, counter);
                        }
                    }
                },
                Err(RecvTimeoutError::Timeout) => {
                    trace!("run: receive timeout");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    debug!("run: sender disconnected");
                    break;
                }
            }

            let mut num_max_writes = max_points;

            let now = Instant::now();
            let host_id = HOST_ID.read().unwrap();
            if now.duration_since(last_write_time) >= write_frequency_secs {
                vec![
                    Level::Error,
                    Level::Warn,
                    Level::Info,
                    Level::Debug,
                    Level::Trace,
                ]
                .iter()
                .for_each(|x| {
                    if let Some((last_time, counters, points)) = points_map.remove(x) {
                        let counter_points = counters.into_iter().map(|(_, v)| v.into());
                        let points: Vec<_> = points.into_iter().chain(counter_points).collect();
                        let num_written = Self::write(
                            &host_id,
                            points,
                            last_time,
                            num_max_writes,
                            writer,
                            max_points_per_sec,
                        );

                        if num_written > 0 {
                            last_write_time = Instant::now();
                        }

                        num_max_writes = num_max_writes.saturating_sub(num_written);
                    }
                });
            }
        }
        trace!("run: exit");
    }

    pub fn submit(&self, mut point: influxdb::Point, level: log::Level) {
        if point.timestamp.is_none() {
            point.add_timestamp(timing::timestamp() as i64);
        }
        debug!("Submitting point: {:?}", point);
        self.sender
            .send(MetricsCommand::Submit(point, level))
            .unwrap();
    }

    pub fn submit_counter(&self, point: CounterPoint, level: log::Level, bucket: u64) {
        debug!("Submitting counter point: {:?}", point);
        self.sender
            .send(MetricsCommand::SubmitCounter(point, level, bucket))
            .unwrap();
    }

    pub fn flush(&self) {
        debug!("Flush");
        let barrier = Arc::new(Barrier::new(2));
        self.sender
            .send(MetricsCommand::Flush(Arc::clone(&barrier)))
            .unwrap();

        barrier.wait();
    }
}

impl Drop for MetricsAgent {
    fn drop(&mut self) {
        self.flush();
    }
}

fn get_singleton_agent() -> Arc<Mutex<MetricsAgent>> {
    static INIT: Once = Once::new();
    static mut AGENT: Option<Arc<Mutex<MetricsAgent>>> = None;
    unsafe {
        INIT.call_once(|| AGENT = Some(Arc::new(Mutex::new(MetricsAgent::default()))));
        match AGENT {
            Some(ref agent) => agent.clone(),
            None => panic!("Failed to initialize metrics agent"),
        }
    }
}

lazy_static! {
    static ref HOST_ID: Arc<RwLock<influx_db_client::Value>> = {
        Arc::new(RwLock::new(influx_db_client::Value::String({
            let hostname: String = hostname().unwrap_or_else(|_| "".to_string());
            format!("{}", hash(hostname.as_bytes())).to_string()
        })))
    };
}

pub fn set_host_id(host_id: String) {
    let mut rw = HOST_ID.write().unwrap();
    info!("host id: {}", host_id);
    std::mem::replace(&mut *rw, influx_db_client::Value::String(host_id));
}

/// Submits a new point from any thread.  Note that points are internally queued
/// and transmitted periodically in batches.
pub fn submit(point: influxdb::Point, level: log::Level) {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.submit(point, level);
}

/// Submits a new counter or updates an existing counter from any thread.  Note that points are
/// internally queued and transmitted periodically in batches.
pub fn submit_counter(point: CounterPoint, level: log::Level, bucket: u64) {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.submit_counter(point, level, bucket);
}

#[derive(Debug, Default)]
struct MetricsConfig {
    pub host: String,
    pub db: String,
    pub username: String,
    pub password: String,
}

impl MetricsConfig {
    fn complete(&self) -> bool {
        !(self.host.is_empty()
            || self.db.is_empty()
            || self.username.is_empty()
            || self.password.is_empty())
    }
}

fn get_metrics_config() -> Result<MetricsConfig, String> {
    let mut config = MetricsConfig::default();

    let config_var = env::var("SOLANA_METRICS_CONFIG")
        .map_err(|err| format!("SOLANA_METRICS_CONFIG: {}", err))?
        .to_string();

    for pair in config_var.split(',') {
        let nv: Vec<_> = pair.split('=').collect();
        if nv.len() != 2 {
            return Err(format!("SOLANA_METRICS_CONFIG is invalid: '{}'", pair));
        }
        let v = nv[1].to_string();
        match nv[0] {
            "host" => config.host = v,
            "db" => config.db = v,
            "u" => config.username = v,
            "p" => config.password = v,
            _ => return Err(format!("SOLANA_METRICS_CONFIG is invalid: '{}'", pair)),
        }
    }

    if !config.complete() {
        return Err("SOLANA_METRICS_CONFIG is incomplete".to_string());
    }
    Ok(config)
}

pub fn query(q: &str) -> Result<String, String> {
    let config = get_metrics_config().map_err(|err| err.to_string())?;
    let query = format!(
        "{}/query?u={}&p={}&q={}",
        &config.host, &config.username, &config.password, &q
    );

    let response = ureq::get(query.as_str())
        .call()
        .into_string()
        .map_err(|err| err.to_string())?;

    Ok(response)
}

/// Blocks until all pending points from previous calls to `submit` have been
/// transmitted.
pub fn flush() {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.flush();
}

/// Hook the panic handler to generate a data point on each panic
pub fn set_panic_hook(program: &'static str) {
    use std::panic;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |ono| {
            default_hook(ono);
            submit(
                influxdb::Point::new("panic")
                    .add_tag("program", influxdb::Value::String(program.to_string()))
                    .add_tag(
                        "thread",
                        influxdb::Value::String(
                            thread::current().name().unwrap_or("?").to_string(),
                        ),
                    )
                    // The 'one' field exists to give Kapacitor Alerts a numerical value
                    // to filter on
                    .add_field("one", influxdb::Value::Integer(1))
                    .add_field(
                        "message",
                        influxdb::Value::String(
                            // TODO: use ono.message() when it becomes stable
                            ono.to_string(),
                        ),
                    )
                    .add_field(
                        "location",
                        influxdb::Value::String(match ono.location() {
                            Some(location) => location.to_string(),
                            None => "?".to_string(),
                        }),
                    )
                    .to_owned(),
                Level::Error,
            );
            // Flush metrics immediately in case the process exits immediately
            // upon return
            flush();
        }));
    });
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json;

    struct MockMetricsWriter {
        points_written: Arc<Mutex<Vec<influxdb::Point>>>,
    }
    impl MockMetricsWriter {
        fn new() -> Self {
            MockMetricsWriter {
                points_written: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn points_written(&self) -> usize {
            self.points_written.lock().unwrap().len()
        }
    }

    impl MetricsWriter for MockMetricsWriter {
        fn write(&self, points: Vec<influxdb::Point>) {
            assert!(!points.is_empty());

            let new_points = points.len();
            self.points_written
                .lock()
                .unwrap()
                .extend(points.into_iter());

            info!(
                "Writing {} points ({} total)",
                new_points,
                self.points_written()
            );
        }
    }

    #[test]
    fn test_submit() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..42 {
            agent.submit(
                influxdb::Point::new(&format!("measurement {}", i)),
                Level::Info,
            );
        }

        agent.flush();
        assert_eq!(writer.points_written(), 42);
    }

    #[test]
    fn test_submit_counter() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..10 {
            agent.submit_counter(CounterPoint::new("counter - 1"), Level::Info, i);
            agent.submit_counter(CounterPoint::new("counter - 2"), Level::Info, i);
        }

        agent.flush();
        assert_eq!(writer.points_written(), 20);
    }

    #[test]
    fn test_submit_counter_increment() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for _ in 0..10 {
            agent.submit_counter(
                CounterPoint {
                    name: "counter",
                    count: 10,
                    timestamp: 0,
                },
                Level::Info,
                0, // use the same bucket
            );
        }

        agent.flush();
        assert_eq!(writer.points_written(), 1);

        let submitted_point = writer.points_written.lock().unwrap()[0].clone();
        let submitted_count = submitted_point.fields.get("count").unwrap();
        let expected_count = &influxdb::Value::Integer(100);
        assert_eq!(
            serde_json::to_string(submitted_count).unwrap(),
            serde_json::to_string(expected_count).unwrap()
        );
    }

    #[test]
    fn test_submit_bucketed_counter() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..50 {
            agent.submit_counter(CounterPoint::new("counter - 1"), Level::Info, i / 10);
            agent.submit_counter(CounterPoint::new("counter - 2"), Level::Info, i / 10);
        }

        agent.flush();
        assert_eq!(writer.points_written(), 10);
    }

    #[test]
    fn test_submit_with_delay() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(1), 1000);

        agent.submit(influxdb::Point::new("point 1"), Level::Info);
        thread::sleep(Duration::from_secs(2));
        assert_eq!(writer.points_written(), 2);
    }

    #[test]
    fn test_submit_exceed_max_rate() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(1), 100);

        for i in 0..102 {
            agent.submit(
                influxdb::Point::new(&format!("measurement {}", i)),
                Level::Info,
            );
        }

        thread::sleep(Duration::from_secs(2));

        agent.flush();
        assert_eq!(writer.points_written(), 100);
    }

    #[test]
    fn test_multithread_submit() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = Arc::new(Mutex::new(MetricsAgent::new(
            writer.clone(),
            Duration::from_secs(10),
            1000,
        )));

        //
        // Submit measurements from different threads
        //
        let mut threads = Vec::new();
        for i in 0..42 {
            let point = influxdb::Point::new(&format!("measurement {}", i));
            let agent = Arc::clone(&agent);
            threads.push(thread::spawn(move || {
                agent.lock().unwrap().submit(point, Level::Info);
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }

        agent.lock().unwrap().flush();
        assert_eq!(writer.points_written(), 42);
    }

    #[test]
    fn test_flush_before_drop() {
        let writer = Arc::new(MockMetricsWriter::new());
        {
            let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(9999999), 1000);
            agent.submit(influxdb::Point::new("point 1"), Level::Info);
        }

        assert_eq!(writer.points_written(), 1);
    }

    #[test]
    fn test_live_submit() {
        let agent = MetricsAgent::default();

        let point = influxdb::Point::new("live_submit_test")
            .add_tag("test", influxdb::Value::Boolean(true))
            .add_field(
                "random_bool",
                influxdb::Value::Boolean(rand::random::<u8>() < 128),
            )
            .add_field(
                "random_int",
                influxdb::Value::Integer(rand::random::<u8>() as i64),
            )
            .to_owned();
        agent.submit(point, Level::Info);
    }

    #[test]
    fn test_datapoint() {
        macro_rules! matches {
            ($e:expr, $p:pat) => {
                match $e {
                    $p => true,
                    _ => false,
                }
            };
        }
        datapoint!("name", ("field name", "test".to_string(), String));
        datapoint!("name", ("field name", 12.34_f64, f64));
        datapoint!("name", ("field name", true, bool));
        datapoint!("name", ("field name", 1, i64));
        datapoint!("name", ("field name", 1, i64),);
        datapoint!("name", ("field1 name", 2, i64), ("field2 name", 2, i64));
        datapoint!("name", ("field1 name", 2, i64), ("field2 name", 2, i64),);
        datapoint!(
            "name",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64)
        );
        datapoint!(
            "name",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64),
        );

        let point = datapoint!(@point "name", ("i64", 1, i64), ("String", "string".to_string(), String), ("f64", 12.34_f64, f64), ("bool", true, bool));
        assert_eq!(point.measurement, "name");
        assert!(matches!(
            point.fields.get("i64").unwrap(),
            influxdb::Value::Integer(1)
        ));
        assert!(match point.fields.get("String").unwrap() {
            influxdb::Value::String(ref s) => {
                if s == "string" {
                    true
                } else {
                    false
                }
            }
            _ => false,
        });
        assert!(match point.fields.get("f64").unwrap() {
            influxdb::Value::Float(f) => {
                if *f == 12.34_f64 {
                    true
                } else {
                    false
                }
            }
            _ => false,
        });
        assert!(matches!(
            point.fields.get("bool").unwrap(),
            influxdb::Value::Boolean(true)
        ));
    }

}
