//! The `metrics` module enables sending measurements to an InfluxDB instance

use influx_db_client as influxdb;
use std::env;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Barrier, Mutex, Once, ONCE_INIT};
use std::thread;
use std::time::{Duration, Instant};
use timing;

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
        InfluxDbMetricsWriter {
            client: Self::build_client(),
        }
    }

    fn build_client() -> Option<influxdb::Client> {
        let host = env::var("INFLUX_HOST").unwrap_or("https://metrics.solana.com:8086".to_string());
        let db = env::var("INFLUX_DATABASE").unwrap_or("scratch".to_string());
        let username = env::var("INFLUX_USERNAME").unwrap_or("scratch_writer".to_string());
        let password = env::var("INFLUX_PASSWORD").unwrap_or("topsecret".to_string());

        debug!("InfluxDB host={} db={} username={}", host, db, username);
        let mut client = influxdb::Client::new_with_option(host, db, None)
            .set_authentication(username, password);

        client.set_read_timeout(1 /*second*/);
        client.set_write_timeout(1 /*second*/);

        debug!("InfluxDB version: {:?}", client.get_version());
        Some(client)
    }
}

impl MetricsWriter for InfluxDbMetricsWriter {
    fn write(&self, points: Vec<influxdb::Point>) {
        if let Some(ref client) = self.client {
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
        thread::spawn(move || Self::run(receiver, writer, write_frequency));
        MetricsAgent { sender }
    }

    fn run(
        receiver: Receiver<MetricsCommand>,
        writer: Arc<MetricsWriter>,
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
            if now.duration_since(last_write_time) >= write_frequency {
                if !points.is_empty() {
                    debug!("run: writing {} points", points.len());
                    writer.write(points);
                    points = Vec::new();
                    last_write_time = now;
                }
            }
        }
        trace!("run: exit");
    }

    pub fn submit(&self, mut point: influxdb::Point) {
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

/// Blocks until all pending points from previous calls to `submit` have been
/// transmitted.
pub fn flush() {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.flush();
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::random;
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

            println!(
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
                influxdb::Value::Boolean(random::<u8>() < 128),
            )
            .add_field(
                "random_int",
                influxdb::Value::Integer(random::<u8>() as i64),
            )
            .to_owned();
        agent.submit(point);
    }

}
