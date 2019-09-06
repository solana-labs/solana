use {Point, Value};

/// Resolve the points to line protocol format
pub(crate) fn line_serialization<T: Iterator<Item = Point>>(points: T) -> String {
    let mut line = Vec::new();
    for point in points {
        line.push(escape_measurement(&point.measurement));

        for (tag, value) in point.tags {
            line.push(",".to_string());
            line.push(escape_keys_and_tags(&tag));
            line.push("=".to_string());

            match value {
                Value::String(s) => line.push(escape_keys_and_tags(&s)),
                Value::Float(f) => line.push(f.to_string()),
                Value::Integer(i) => line.push(i.to_string() + "i"),
                Value::Boolean(b) => line.push({
                    if b {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }),
            }
        }

        let mut was_first = true;

        for (field, value) in point.fields {
            line.push(
                {
                    if was_first {
                        was_first = false;
                        " "
                    } else {
                        ","
                    }
                }.to_string(),
            );
            line.push(escape_keys_and_tags(&field));
            line.push("=".to_string());

            match value {
                Value::String(s) => line.push(escape_string_field_value(
                    &s.replace("\\\"", "\\\\\""),
                )),
                Value::Float(f) => line.push(f.to_string()),
                Value::Integer(i) => line.push(i.to_string() + "i"),
                Value::Boolean(b) => line.push({
                    if b {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }),
            }
        }

        if let Some(t) = point.timestamp {
            line.push(" ".to_string());
            line.push(t.to_string());
        }

        line.push("\n".to_string())
    }

    line.join("")
}

#[inline]
pub(crate) fn quote_ident(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
    )
}

#[inline]
pub(crate) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace("\\", "\\\\").replace("'", "\\'"))
}

#[inline]
pub(crate) fn conversion(value: &str) -> String {
    value
        .replace("\'", "")
        .replace("\"", "")
        .replace("\\", "")
        .trim()
        .to_string()
}

#[inline]
fn escape_keys_and_tags(value: &str) -> String {
    value
        .replace(",", "\\,")
        .replace("=", "\\=")
        .replace(" ", "\\ ")
}

#[inline]
fn escape_measurement(value: &str) -> String {
    value.replace(",", "\\,").replace(" ", "\\ ")
}

#[inline]
fn escape_string_field_value(value: &str) -> String {
    format!("\"{}\"", value.replace("\"", "\\\""))
}

#[cfg(test)]
mod test {
    use super::*;
    use {Point, Points};

    #[test]
    fn line_serialization_test() {
        let mut point = Point::new("test");
        point.add_field("somefield", Value::Integer(65));
        point.add_tag("sometag", Value::Boolean(false));
        let points = Points::new(point);

        assert_eq!(
            line_serialization(points),
            "test,sometag=false somefield=65i\n"
        )
    }

    #[test]
    fn escape_keys_and_tags_test() {
        assert_eq!(
            escape_keys_and_tags("foo, hello=world"),
            "foo\\,\\ hello\\=world"
        )
    }

    #[test]
    fn escape_measurement_test() {
        assert_eq!(escape_measurement("foo, hello"), "foo\\,\\ hello")
    }

    #[test]
    fn escape_string_field_value_test() {
        assert_eq!(escape_string_field_value("\"foo"), "\"\\\"foo\"")
    }

    #[test]
    fn quote_ident_test() {
        assert_eq!(quote_ident("root"), "\"root\"")
    }

    #[test]
    fn quote_literal_test() {
        assert_eq!(quote_literal("root"), "\'root\'")
    }
}
