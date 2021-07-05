use std::fmt;

#[derive(Clone, Debug)]
pub struct DataPoint {
    pub name: &'static str,
    pub timestamp: u64,
    pub fields: Vec<(&'static str, String)>,
}

impl DataPoint {
    pub fn new(name: &'static str) -> Self {
        DataPoint {
            name,
            timestamp: solana_sdk::timing::timestamp(),
            fields: vec![],
        }
    }

    pub fn add_field_str(&mut self, name: &'static str, value: &str) -> &mut Self {
        self.fields
            .push((name, format!("\"{}\"", value.replace("\"", "\\\""))));
        self
    }

    pub fn add_field_bool(&mut self, name: &'static str, value: bool) -> &mut Self {
        self.fields.push((name, value.to_string()));
        self
    }

    pub fn add_field_i64(&mut self, name: &'static str, value: i64) -> &mut Self {
        self.fields.push((name, value.to_string() + "i"));
        self
    }

    pub fn add_field_f64(&mut self, name: &'static str, value: f64) -> &mut Self {
        self.fields.push((name, value.to_string()));
        self
    }
}

impl fmt::Display for DataPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "datapoint: {}", self.name)?;
        for field in &self.fields {
            write!(f, " {}={}", field.0, field.1)?;
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! create_datapoint {
    (@field $point:ident $name:expr, $string:expr, String) => {
        $point.add_field_str($name, &$string);
    };
    (@field $point:ident $name:expr, $value:expr, i64) => {
        $point.add_field_i64($name, $value as i64);
    };
    (@field $point:ident $name:expr, $value:expr, f64) => {
        $point.add_field_f64($name, $value as f64);
    };
    (@field $point:ident $name:expr, $value:expr, bool) => {
        $point.add_field_bool($name, $value as bool);
    };

    (@fields $point:ident) => {};
    (@fields $point:ident ($name:expr, $value:expr, $type:ident) , $($rest:tt)*) => {
        $crate::create_datapoint!(@field $point $name, $value, $type);
        $crate::create_datapoint!(@fields $point $($rest)*);
    };
    (@fields $point:ident ($name:expr, $value:expr, $type:ident)) => {
        $crate::create_datapoint!(@field $point $name, $value, $type);
    };

    (@point $name:expr, $($fields:tt)+) => {
        {
            let mut point = $crate::datapoint::DataPoint::new(&$name);
            $crate::create_datapoint!(@fields point $($fields)+);
            point
        }
    };
    (@point $name:expr) => {
        $crate::datapoint::DataPoint::new(&$name)
    };
}

#[macro_export]
macro_rules! datapoint {
    ($level:expr, $name:expr) => {
        if log::log_enabled!($level) {
            $crate::submit($crate::create_datapoint!(@point $name), $level);
        }
    };
    ($level:expr, $name:expr, $($fields:tt)+) => {
        if log::log_enabled!($level) {
            $crate::submit($crate::create_datapoint!(@point $name, $($fields)+), $level);
        }
    };
}
#[macro_export]
macro_rules! datapoint_error {
    ($name:expr) => {
        $crate::datapoint!(log::Level::Error, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Error, $name, $($fields)+);
    };
}

#[macro_export]
macro_rules! datapoint_warn {
    ($name:expr) => {
        $crate::datapoint!(log::Level::Warn, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Warn, $name, $($fields)+);
    };
}

#[macro_export]
macro_rules! datapoint_info {
    ($name:expr) => {
        $crate::datapoint!(log::Level::Info, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Info, $name, $($fields)+);
    };
}

#[macro_export]
macro_rules! datapoint_debug {
    ($name:expr) => {
        $crate::datapoint!(log::Level::Debug, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Debug, $name, $($fields)+);
    };
}

#[macro_export]
macro_rules! datapoint_trace {
    ($name:expr) => {
        $crate::datapoint!(log::Level::Trace, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Trace, $name, $($fields)+);
    };
}

#[cfg(test)]
mod test {
    #[test]
    fn test_datapoint() {
        datapoint_debug!("name", ("field name", "test".to_string(), String));
        datapoint_info!("name", ("field name", 12.34_f64, f64));
        datapoint_trace!("name", ("field name", true, bool));
        datapoint_warn!("name", ("field name", 1, i64));
        datapoint_error!("name", ("field name", 1, i64),);
        datapoint!(
            log::Level::Warn,
            "name",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64)
        );
        datapoint_info!("name", ("field1 name", 2, i64), ("field2 name", 2, i64),);
        datapoint_trace!(
            "name",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64)
        );
        datapoint!(
            log::Level::Error,
            "name",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64),
        );

        let point = create_datapoint!(
            @point "name",
            ("i64", 1, i64),
            ("String", "string space string".to_string(), String),
            ("f64", 12.34_f64, f64),
            ("bool", true, bool)
        );
        assert_eq!(point.name, "name");
        assert_eq!(point.fields[0], ("i64", "1i".to_string()));
        assert_eq!(
            point.fields[1],
            ("String", "\"string space string\"".to_string())
        );
        assert_eq!(point.fields[2], ("f64", "12.34".to_string()));
        assert_eq!(point.fields[3], ("bool", "true".to_string()));
    }
}
