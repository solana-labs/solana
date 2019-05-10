//! The `metrics` module enables sending measurements to an `InfluxDB` instance

use influx_db_client as influxdb;
use lazy_static::lazy_static;
use log::*;
use solana_sdk::hash::hash;
use solana_sdk::timing;
use std::env;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Barrier, Mutex, Once, ONCE_INIT};
use std::thread;
use std::time::{Duration, Instant};
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

    ($name:expr, $($fields:tt)+) => {
        $crate::submit($crate::datapoint!(@point $name, $($fields)+));
    };
}

lazy_static! {
    static ref HOST_INFO: String = {
        let v = env::var("SOLANA_METRICS_DISPLAY_HOSTNAME")
            .map(|x| x.parse().unwrap_or(0))
            .unwrap_or(0);
        let name: String = hostname().unwrap_or_else(|_| "".to_string());
        if v == 0 {
            hash(name.as_bytes()).to_string()
        } else {
            name
        }
    };
}

#[derive(Debug)]
enum MetricsCommand {
    Submit(influxdb::Point),
    Flush(Arc<Barrier>),
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

    fn build_client() -> Result<influxdb::Client, env::VarError> {
        let (host, db, username, password) = get_env_settings()?;

        debug!("InfluxDB host={} db={} username={}", host, db, username);
        let mut client = influxdb::Client::new_with_option(host, db, None)
            .set_authentication(username, password);

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
        Self::new(
            Arc::new(InfluxDbMetricsWriter::new()),
            Duration::from_secs(10),
        )
    }
}

impl MetricsAgent {
    fn new(writer: Arc<MetricsWriter + Send + Sync>, write_frequency: Duration) -> Self {
        let (sender, receiver) = channel::<MetricsCommand>();
        thread::spawn(move || Self::run(&receiver, &writer, write_frequency));
        Self { sender }
    }

    fn run(
        receiver: &Receiver<MetricsCommand>,
        writer: &Arc<MetricsWriter + Send + Sync>,
        write_frequency: Duration,
    ) {
        trace!("run: enter");
        let mut last_write_time = Instant::now();
        let mut points = Vec::new();

        loop {
            match receiver.recv_timeout(write_frequency / 2) {
                Ok(cmd) => match cmd {
                    MetricsCommand::Flush(barrier) => {
                        debug!("metrics_thread: flush");
                        if !points.is_empty() {
                            writer.write(points);
                            points = Vec::new();
                            last_write_time = Instant::now();
                        }
                        barrier.wait();
                    }
                    MetricsCommand::Submit(point) => {
                        debug!("run: submit {:?}", point);
                        points.push(point);
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

            let now = Instant::now();
            if now.duration_since(last_write_time) >= write_frequency && !points.is_empty() {
                debug!("run: writing {} points", points.len());
                writer.write(points);
                points = Vec::new();
                last_write_time = now;
            }
        }
        trace!("run: exit");
    }

    pub fn submit(&self, mut point: influxdb::Point) {
        point.add_field("host_id", influxdb::Value::String(HOST_INFO.to_string()));
        if point.timestamp.is_none() {
            point.timestamp = Some(timing::timestamp() as i64);
        }
        debug!("Submitting point: {:?}", point);
        self.sender.send(MetricsCommand::Submit(point)).unwrap();
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
    static INIT: Once = ONCE_INIT;
    static mut AGENT: Option<Arc<Mutex<MetricsAgent>>> = None;
    unsafe {
        INIT.call_once(|| AGENT = Some(Arc::new(Mutex::new(MetricsAgent::default()))));
        match AGENT {
            Some(ref agent) => agent.clone(),
            None => panic!("Failed to initialize metrics agent"),
        }
    }
}

/// Submits a new point from any thread.  Note that points are internally queued
/// and transmitted periodically in batches.
pub fn submit(point: influxdb::Point) {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.submit(point);
}

fn get_env_settings() -> Result<(String, String, String, String), env::VarError> {
    let host =
        env::var("INFLUX_HOST").unwrap_or_else(|_| "https://metrics.solana.com:8086".to_string());
    let db = env::var("INFLUX_DATABASE")?.to_string();
    let username = env::var("INFLUX_USERNAME")?.to_string();
    let password = env::var("INFLUX_PASSWORD")?.to_string();
    Ok((host, db, username, password))
}

pub fn query(q: &str) -> Result<String, String> {
    let (host, _db, username, password) = get_env_settings().map_err(|err| err.to_string())?;
    let query = format!("{}/query?u={}&p={}&q={}", &host, &username, &password, &q);

    let response = reqwest::get(query.as_str())
        .map_err(|err| err.to_string())?
        .text()
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
    static SET_HOOK: Once = ONCE_INIT;
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
                    .add_field("host_id", influxdb::Value::String(HOST_INFO.to_string()))
                    .to_owned(),
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockMetricsWriter {
        points_written: AtomicUsize,
    }
    impl MockMetricsWriter {
        fn new() -> Self {
            MockMetricsWriter {
                points_written: AtomicUsize::new(0),
            }
        }

        fn points_written(&self) -> usize {
            return self.points_written.load(Ordering::SeqCst);
        }
    }

    impl MetricsWriter for MockMetricsWriter {
        fn write(&self, points: Vec<influxdb::Point>) {
            assert!(!points.is_empty());

            self.points_written
                .fetch_add(points.len(), Ordering::SeqCst);

            info!(
                "Writing {} points ({} total)",
                points.len(),
                self.points_written.load(Ordering::SeqCst)
            );
        }
    }

    #[test]
    fn test_submit() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10));

        for i in 0..42 {
            agent.submit(influxdb::Point::new(&format!("measurement {}", i)));
        }

        agent.flush();
        assert_eq!(writer.points_written(), 42);
    }

    #[test]
    fn test_submit_with_delay() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_millis(100));

        agent.submit(influxdb::Point::new("point 1"));
        thread::sleep(Duration::from_secs(2));
        assert_eq!(writer.points_written(), 1);
    }

    #[test]
    fn test_multithread_submit() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = Arc::new(Mutex::new(MetricsAgent::new(
            writer.clone(),
            Duration::from_secs(10),
        )));

        //
        // Submit measurements from different threads
        //
        let mut threads = Vec::new();
        for i in 0..42 {
            let point = influxdb::Point::new(&format!("measurement {}", i));
            let agent = Arc::clone(&agent);
            threads.push(thread::spawn(move || {
                agent.lock().unwrap().submit(point);
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
            let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(9999999));
            agent.submit(influxdb::Point::new("point 1"));
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
        agent.submit(point);
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
