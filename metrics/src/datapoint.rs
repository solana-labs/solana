//! This file defines a set of macros for reporting metrics.
//!
//! To report a metric, simply calling one of the following datapoint macros
//! with a suitable message level:
//!
//! - datapoint_error!
//! - datapoint_warn!
//! - datapoint_trace!
//! - datapoint_info!
//! - datapoint_debug!
//!
//! The matric macro consists of the following three main parts:
//!  - name: the name of the metric.
//!
//!  - tags (optional): when a metric sample is reported with tags, you can use
//!    group-by when querying the reported samples.  Each metric sample can be
//!    attached with zero to many tags.  Each tag is of the format:
//!
//!    - "tag-name" => "tag-value"
//!
//!  - fields (optional): fields are the main content of a metric sample. The
//!    macro supports four different types of fields: bool, i64, f64, and String.
//!    Here're their syntax:
//!
//!    - ("field-name", "field-value", bool)
//!    - ("field-name", "field-value", i64)
//!    - ("field-name", "field-value", f64)
//!    - ("field-name", "field-value", String)
//!
//! Example:
//!
//! datapoint_debug!(
//!     "name-of-the-metric",
//!     "tag" => "tag-value",
//!     "tag2" => "tag-value2",
//!     ("some-bool", false, bool),
//!     ("some-int", 100, i64),
//!     ("some-float", 1.05, f64),
//!     ("some-string", "field-value", String),
//! );
//!
use std::{fmt, time::SystemTime};

#[derive(Clone, Debug)]
pub struct DataPoint {
    pub name: &'static str,
    pub timestamp: SystemTime,
    /// tags are eligible for group-by operations.
    pub tags: Vec<(&'static str, String)>,
    pub fields: Vec<(&'static str, String)>,
}

impl DataPoint {
    pub fn new(name: &'static str) -> Self {
        DataPoint {
            name,
            timestamp: SystemTime::now(),
            tags: vec![],
            fields: vec![],
        }
    }

    pub fn add_tag(&mut self, name: &'static str, value: &str) -> &mut Self {
        self.tags.push((name, value.to_string()));
        self
    }

    pub fn add_field_str(&mut self, name: &'static str, value: &str) -> &mut Self {
        self.fields
            .push((name, format!("\"{}\"", value.replace('\"', "\\\""))));
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
        for tag in &self.tags {
            write!(f, ",{}={}", tag.0, tag.1)?;
        }
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
    (@tag $point:ident $tag_name:expr, $tag_value:expr) => {
        $point.add_tag($tag_name, &$tag_value);
    };

    (@fields $point:ident) => {};

    // process optional fields
    (@fields $point:ident ($name:expr, $value:expr, Option<$type:ident>) , $($rest:tt)*) => {
        if let Some(value) = $value {
            $crate::create_datapoint!(@field $point $name, value, $type);
        }
        $crate::create_datapoint!(@fields $point $($rest)*);
    };
    (@fields $point:ident ($name:expr, $value:expr, Option<$type:ident>) $(,)?) => {
        if let Some(value) = $value {
            $crate::create_datapoint!(@field $point $name, value, $type);
        }
    };

    // process tags
    (@fields $point:ident $tag_name:expr => $tag_value:expr, $($rest:tt)*) => {
        $crate::create_datapoint!(@tag $point $tag_name, $tag_value);
        $crate::create_datapoint!(@fields $point $($rest)*);
    };
    (@fields $point:ident $tag_name:expr => $tag_value:expr $(,)?) => {
        $crate::create_datapoint!(@tag $point $tag_name, $tag_value);
    };

    // process fields
    (@fields $point:ident ($name:expr, $value:expr, $type:ident) , $($rest:tt)*) => {
        $crate::create_datapoint!(@field $point $name, $value, $type);
        $crate::create_datapoint!(@fields $point $($rest)*);
    };
    (@fields $point:ident ($name:expr, $value:expr, $type:ident) $(,)?) => {
        $crate::create_datapoint!(@field $point $name, $value, $type);
    };

    (@point $name:expr, $($fields:tt)+) => {
        {
            let mut point = $crate::datapoint::DataPoint::new(&$name);
            $crate::create_datapoint!(@fields point $($fields)+);
            point
        }
    };
    (@point $name:expr $(,)?) => {
        $crate::datapoint::DataPoint::new(&$name)
    };
}

#[macro_export]
macro_rules! datapoint {
    ($level:expr, $name:expr $(,)?) => {
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
    ($name:expr $(,)?) => {
        $crate::datapoint!(log::Level::Error, $name);
    };
    ($name:expr, $($fields:tt)+) => {
        $crate::datapoint!(log::Level::Error, $name, $($fields)+);
    };
}

#[macro_export]
macro_rules! datapoint_warn {
    ($name:expr $(,)?) => {
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
        datapoint_debug!("name", ("field name", "test", String));
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
            ("String", "string space string", String),
            ("f64", 12.34_f64, f64),
            ("bool", true, bool)
        );
        assert_eq!(point.name, "name");
        assert_eq!(point.tags.len(), 0);
        assert_eq!(point.fields[0], ("i64", "1i".to_string()));
        assert_eq!(
            point.fields[1],
            ("String", "\"string space string\"".to_string())
        );
        assert_eq!(point.fields[2], ("f64", "12.34".to_string()));
        assert_eq!(point.fields[3], ("bool", "true".to_string()));
    }

    #[test]
    fn test_optional_datapoint() {
        datapoint_debug!("name", ("field name", Some("test"), Option<String>));
        datapoint_info!("name", ("field name", Some(12.34_f64), Option<f64>));
        datapoint_trace!("name", ("field name", Some(true), Option<bool>));
        datapoint_warn!("name", ("field name", Some(1), Option<i64>));
        datapoint_error!("name", ("field name", Some(1), Option<i64>),);
        datapoint_debug!("name", ("field name", None::<String>, Option<String>));
        datapoint_info!("name", ("field name", None::<f64>, Option<f64>));
        datapoint_trace!("name", ("field name", None::<bool>, Option<bool>));
        datapoint_warn!("name", ("field name", None::<i64>, Option<i64>));
        datapoint_error!("name", ("field name", None::<i64>, Option<i64>),);

        let point = create_datapoint!(
            @point "name",
            ("some_i64", Some(1), Option<i64>),
            ("no_i64", None::<i64>, Option<i64>),
            ("some_String", Some("string space string"), Option<String>),
            ("no_String", None::<String>, Option<String>),
            ("some_f64", Some(12.34_f64), Option<f64>),
            ("no_f64", None::<f64>, Option<f64>),
            ("some_bool", Some(true), Option<bool>),
            ("no_bool", None::<bool>, Option<bool>),
        );
        assert_eq!(point.name, "name");
        assert_eq!(point.tags.len(), 0);
        assert_eq!(point.fields[0], ("some_i64", "1i".to_string()));
        assert_eq!(
            point.fields[1],
            ("some_String", "\"string space string\"".to_string())
        );
        assert_eq!(point.fields[2], ("some_f64", "12.34".to_string()));
        assert_eq!(point.fields[3], ("some_bool", "true".to_string()));
        assert_eq!(point.fields.len(), 4);
    }

    #[test]
    fn test_datapoint_with_tags() {
        datapoint_debug!("name", "tag" => "tag-value", ("field name", "test", String));
        datapoint_info!(
            "name",
            "tag" => "tag-value",
            "tag2" => "tag-value-2",
            ("field name", 12.34_f64, f64)
        );
        datapoint_trace!(
            "name",
            "tag" => "tag-value",
            "tag2" => "tag-value-2",
            "tag3" => "tag-value-3",
            ("field name", true, bool)
        );
        datapoint_warn!("name", "tag" => "tag-value");
        datapoint_error!("name", "tag" => "tag-value", ("field name", 1, i64),);
        datapoint!(
            log::Level::Warn,
            "name",
            "tag" => "tag-value",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64)
        );
        datapoint_info!("name", ("field1 name", 2, i64), ("field2 name", 2, i64),);
        datapoint_trace!(
            "name",
            "tag" => "tag-value",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64)
        );
        datapoint!(
            log::Level::Error,
            "name",
            "tag" => "tag-value",
            ("field1 name", 2, i64),
            ("field2 name", 2, i64),
            ("field3 name", 3, i64),
        );

        let point = create_datapoint!(
            @point "name",
            "tag1" => "tag-value-1",
            "tag2" => "tag-value-2",
            "tag3" => "tag-value-3",
            ("i64", 1, i64),
            ("String", "string space string", String),
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
        assert_eq!(point.tags[0], ("tag1", "tag-value-1".to_string()));
        assert_eq!(point.tags[1], ("tag2", "tag-value-2".to_string()));
        assert_eq!(point.tags[2], ("tag3", "tag-value-3".to_string()));
    }
}
