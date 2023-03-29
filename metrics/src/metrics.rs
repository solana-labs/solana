//! The `metrics` module enables sending measurements to an `InfluxDB` instance

use {
    crate::{counter::CounterPoint, datapoint::DataPoint},
    crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender},
    gethostname::gethostname,
    lazy_static::lazy_static,
    log::*,
    solana_sdk::hash::hash,
    std::{
        cmp,
        collections::HashMap,
        convert::Into,
        env,
        fmt::Write,
        sync::{Arc, Barrier, Mutex, Once, RwLock},
        thread,
        time::{Duration, Instant, UNIX_EPOCH},
    },
};

type CounterMap = HashMap<(&'static str, u64), CounterPoint>;

impl From<&CounterPoint> for DataPoint {
    fn from(counter_point: &CounterPoint) -> Self {
        let mut point = Self::new(counter_point.name);
        point.timestamp = counter_point.timestamp;
        point.add_field_i64("count", counter_point.count);
        point
    }
}

#[derive(Debug)]
enum MetricsCommand {
    Flush(Arc<Barrier>),
    Submit(DataPoint, log::Level),
    SubmitCounter(CounterPoint, log::Level, u64),
}

pub struct MetricsAgent {
    sender: Sender<MetricsCommand>,
}

pub trait MetricsWriter {
    // Write the points and empty the vector.  Called on the internal
    // MetricsAgent worker thread.
    fn write(&self, points: Vec<DataPoint>);
}

struct InfluxDbMetricsWriter {
    write_url: Option<String>,
}

impl InfluxDbMetricsWriter {
    fn new() -> Self {
        Self {
            write_url: Self::build_write_url().ok(),
        }
    }

    fn build_write_url() -> Result<String, String> {
        let config = get_metrics_config().map_err(|err| {
            info!("metrics disabled: {}", err);
            err
        })?;

        info!(
            "metrics configuration: host={} db={} username={}",
            config.host, config.db, config.username
        );

        let write_url = format!(
            "{}/write?db={}&u={}&p={}&precision=n",
            &config.host, &config.db, &config.username, &config.password
        );

        Ok(write_url)
    }
}

pub fn serialize_points(points: &Vec<DataPoint>, host_id: &str) -> String {
    const TIMESTAMP_LEN: usize = 20;
    const HOST_ID_LEN: usize = 8; // "host_id=".len()
    const EXTRA_LEN: usize = 2; // "=,".len()
    let mut len = 0;
    for point in points {
        for (name, value) in &point.fields {
            len += name.len() + value.len() + EXTRA_LEN;
        }
        for (name, value) in &point.tags {
            len += name.len() + value.len() + EXTRA_LEN;
        }
        len += point.name.len();
        len += TIMESTAMP_LEN;
        len += host_id.len() + HOST_ID_LEN;
    }
    let mut line = String::with_capacity(len);
    for point in points {
        let _ = write!(line, "{},host_id={}", &point.name, host_id);
        for (name, value) in point.tags.iter() {
            let _ = write!(line, ",{name}={value}");
        }

        let mut first = true;
        for (name, value) in point.fields.iter() {
            let _ = write!(line, "{}{}={}", if first { ' ' } else { ',' }, name, value);
            first = false;
        }
        let timestamp = point.timestamp.duration_since(UNIX_EPOCH);
        let nanos = timestamp.unwrap().as_nanos();
        let _ = writeln!(line, " {nanos}");
    }
    line
}

impl MetricsWriter for InfluxDbMetricsWriter {
    fn write(&self, points: Vec<DataPoint>) {
        if let Some(ref write_url) = self.write_url {
            debug!("submitting {} points", points.len());

            let host_id = HOST_ID.read().unwrap();

            let line = serialize_points(&points, &host_id);

            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build();
            let client = match client {
                Ok(client) => client,
                Err(err) => {
                    warn!("client instantiation failed: {}", err);
                    return;
                }
            };

            let response = client.post(write_url.as_str()).body(line).send();
            if let Ok(resp) = response {
                let status = resp.status();
                if !status.is_success() {
                    let text = resp
                        .text()
                        .unwrap_or_else(|_| "[text body empty]".to_string());
                    warn!("submit response unsuccessful: {} {}", status, text,);
                }
            } else {
                warn!("submit error: {}", response.unwrap_err());
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
    pub fn new(
        writer: Arc<dyn MetricsWriter + Send + Sync>,
        write_frequency: Duration,
        max_points_per_sec: usize,
    ) -> Self {
        let (sender, receiver) = unbounded::<MetricsCommand>();

        thread::Builder::new()
            .name("solMetricsAgent".into())
            .spawn(move || Self::run(&receiver, &writer, write_frequency, max_points_per_sec))
            .unwrap();

        Self { sender }
    }

    fn collect_points(points: &mut Vec<DataPoint>, counters: &mut CounterMap) -> Vec<DataPoint> {
        let mut ret = std::mem::take(points);
        ret.extend(counters.values().map(|v| v.into()));
        counters.clear();
        ret
    }

    fn write(
        writer: &Arc<dyn MetricsWriter + Send + Sync>,
        mut points: Vec<DataPoint>,
        max_points: usize,
        max_points_per_sec: usize,
        last_write_time: Instant,
        points_buffered: usize,
    ) {
        if points.is_empty() {
            return;
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
        points.push(
            DataPoint::new("metrics")
                .add_field_i64("points_written", points_written as i64)
                .add_field_i64("num_points", num_points as i64)
                .add_field_i64("points_lost", (num_points - points_written) as i64)
                .add_field_i64("points_buffered", points_buffered as i64)
                .add_field_i64(
                    "secs_since_last_write",
                    now.duration_since(last_write_time).as_secs() as i64,
                )
                .to_owned(),
        );

        writer.write(points);
    }

    fn run(
        receiver: &Receiver<MetricsCommand>,
        writer: &Arc<dyn MetricsWriter + Send + Sync>,
        write_frequency: Duration,
        max_points_per_sec: usize,
    ) {
        trace!("run: enter");
        let mut last_write_time = Instant::now();
        let mut points = Vec::<DataPoint>::new();
        let mut counters = CounterMap::new();

        let max_points = write_frequency.as_secs() as usize * max_points_per_sec;

        loop {
            match receiver.recv_timeout(write_frequency / 2) {
                Ok(cmd) => match cmd {
                    MetricsCommand::Flush(barrier) => {
                        debug!("metrics_thread: flush");
                        Self::write(
                            writer,
                            Self::collect_points(&mut points, &mut counters),
                            max_points,
                            max_points_per_sec,
                            last_write_time,
                            receiver.len(),
                        );
                        last_write_time = Instant::now();
                        barrier.wait();
                    }
                    MetricsCommand::Submit(point, level) => {
                        log!(level, "{}", point);
                        points.push(point);
                    }
                    MetricsCommand::SubmitCounter(counter, _level, bucket) => {
                        debug!("{:?}", counter);
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

            let now = Instant::now();
            if now.duration_since(last_write_time) >= write_frequency {
                Self::write(
                    writer,
                    Self::collect_points(&mut points, &mut counters),
                    max_points,
                    max_points_per_sec,
                    last_write_time,
                    receiver.len(),
                );
                last_write_time = now;
            }
        }
        trace!("run: exit");
    }

    pub fn submit(&self, point: DataPoint, level: log::Level) {
        self.sender
            .send(MetricsCommand::Submit(point, level))
            .unwrap();
    }

    pub fn submit_counter(&self, counter: CounterPoint, level: log::Level, bucket: u64) {
        self.sender
            .send(MetricsCommand::SubmitCounter(counter, level, bucket))
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
    static ref HOST_ID: Arc<RwLock<String>> = {
        Arc::new(RwLock::new({
            let hostname: String = gethostname()
                .into_string()
                .unwrap_or_else(|_| "".to_string());
            format!("{}", hash(hostname.as_bytes()))
        }))
    };
}

pub fn set_host_id(host_id: String) {
    info!("host id: {}", host_id);
    *HOST_ID.write().unwrap() = host_id;
}

/// Submits a new point from any thread.  Note that points are internally queued
/// and transmitted periodically in batches.
pub fn submit(point: DataPoint, level: log::Level) {
    let agent_mutex = get_singleton_agent();
    let agent = agent_mutex.lock().unwrap();
    agent.submit(point, level);
}

/// Submits a new counter or updates an existing counter from any thread.  Note that points are
/// internally queued and transmitted periodically in batches.
pub(crate) fn submit_counter(point: CounterPoint, level: log::Level, bucket: u64) {
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

    let config_var =
        env::var("SOLANA_METRICS_CONFIG").map_err(|err| format!("SOLANA_METRICS_CONFIG: {err}"))?;

    for pair in config_var.split(',') {
        let nv: Vec<_> = pair.split('=').collect();
        if nv.len() != 2 {
            return Err(format!("SOLANA_METRICS_CONFIG is invalid: '{pair}'"));
        }
        let v = nv[1].to_string();
        match nv[0] {
            "host" => config.host = v,
            "db" => config.db = v,
            "u" => config.username = v,
            "p" => config.password = v,
            _ => return Err(format!("SOLANA_METRICS_CONFIG is invalid: '{pair}'")),
        }
    }

    if !config.complete() {
        return Err("SOLANA_METRICS_CONFIG is incomplete".to_string());
    }
    Ok(config)
}

pub fn query(q: &str) -> Result<String, String> {
    let config = get_metrics_config()?;
    let query_url = format!(
        "{}/query?u={}&p={}&q={}",
        &config.host, &config.username, &config.password, &q
    );

    let response = reqwest::blocking::get(query_url.as_str())
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
pub fn set_panic_hook(program: &'static str, version: Option<String>) {
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |ono| {
            default_hook(ono);
            let location = match ono.location() {
                Some(location) => location.to_string(),
                None => "?".to_string(),
            };
            submit(
                DataPoint::new("panic")
                    .add_field_str("program", program)
                    .add_field_str("thread", thread::current().name().unwrap_or("?"))
                    // The 'one' field exists to give Kapacitor Alerts a numerical value
                    // to filter on
                    .add_field_i64("one", 1)
                    .add_field_str("message", &ono.to_string())
                    .add_field_str("location", &location)
                    .add_field_str("version", version.as_ref().unwrap_or(&"".to_string()))
                    .to_owned(),
                Level::Error,
            );
            // Flush metrics immediately
            flush();

            // Exit cleanly so the process don't limp along in a half-dead state
            std::process::exit(1);
        }));
    });
}

pub mod test_mocks {
    use super::*;

    pub struct MockMetricsWriter {
        pub points_written: Arc<Mutex<Vec<DataPoint>>>,
    }
    impl MockMetricsWriter {
        #[allow(dead_code)]
        pub fn new() -> Self {
            MockMetricsWriter {
                points_written: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn points_written(&self) -> usize {
            self.points_written.lock().unwrap().len()
        }
    }

    impl Default for MockMetricsWriter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MetricsWriter for MockMetricsWriter {
        fn write(&self, points: Vec<DataPoint>) {
            assert!(!points.is_empty());

            let new_points = points.len();
            self.points_written
                .lock()
                .unwrap()
                .extend(points.into_iter());

            info!(
                "Writing {} points ({} total)",
                new_points,
                self.points_written(),
            );
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, test_mocks::MockMetricsWriter};

    #[test]
    fn test_submit() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..42 {
            agent.submit(
                DataPoint::new("measurement")
                    .add_field_i64("i", i)
                    .to_owned(),
                Level::Info,
            );
        }

        agent.flush();
        assert_eq!(writer.points_written(), 43);
    }

    #[test]
    fn test_submit_counter() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..10 {
            agent.submit_counter(CounterPoint::new("counter 1"), Level::Info, i);
            agent.submit_counter(CounterPoint::new("counter 2"), Level::Info, i);
        }

        agent.flush();
        assert_eq!(writer.points_written(), 21);
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
                    timestamp: UNIX_EPOCH,
                },
                Level::Info,
                0, // use the same bucket
            );
        }

        agent.flush();
        assert_eq!(writer.points_written(), 2);

        let submitted_point = writer.points_written.lock().unwrap()[0].clone();
        assert_eq!(submitted_point.fields[0], ("count", "100i".to_string()));
    }

    #[test]
    fn test_submit_bucketed_counter() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(10), 1000);

        for i in 0..50 {
            agent.submit_counter(CounterPoint::new("counter 1"), Level::Info, i / 10);
            agent.submit_counter(CounterPoint::new("counter 2"), Level::Info, i / 10);
        }

        agent.flush();
        assert_eq!(writer.points_written(), 11);
    }

    #[test]
    fn test_submit_with_delay() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(1), 1000);

        agent.submit(DataPoint::new("point 1"), Level::Info);
        thread::sleep(Duration::from_secs(2));
        assert_eq!(writer.points_written(), 2);
    }

    #[test]
    fn test_submit_exceed_max_rate() {
        let writer = Arc::new(MockMetricsWriter::new());
        let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(1), 100);

        for i in 0..102 {
            agent.submit(
                DataPoint::new("measurement")
                    .add_field_i64("i", i)
                    .to_owned(),
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
            let mut point = DataPoint::new("measurement");
            point.add_field_i64("i", i);
            let agent = Arc::clone(&agent);
            threads.push(thread::spawn(move || {
                agent.lock().unwrap().submit(point, Level::Info);
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }

        agent.lock().unwrap().flush();
        assert_eq!(writer.points_written(), 43);
    }

    #[test]
    fn test_flush_before_drop() {
        let writer = Arc::new(MockMetricsWriter::new());
        {
            let agent = MetricsAgent::new(writer.clone(), Duration::from_secs(9_999_999), 1000);
            agent.submit(DataPoint::new("point 1"), Level::Info);
        }

        assert_eq!(writer.points_written(), 2);
    }

    #[test]
    fn test_live_submit() {
        let agent = MetricsAgent::default();

        let point = DataPoint::new("live_submit_test")
            .add_field_bool("true", true)
            .add_field_bool("random_bool", rand::random::<u8>() < 128)
            .add_field_i64("random_int", rand::random::<u8>() as i64)
            .to_owned();
        agent.submit(point, Level::Info);
    }
}
