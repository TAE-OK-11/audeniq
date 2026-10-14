//! Bounded RFC-style CSV records for normalized royalty imports.
//! Quoted commas, escaped quotes and embedded CRLF are preserved. Malformed
//! quotes are rejected rather than silently changing monetary columns.

pub(crate) fn records(input: &str) -> Result<Vec<Vec<String>>, &'static str> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("REPORT_TOO_LARGE");
    }
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut chars = input.chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut started = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if !started && field.is_empty() => {
                quoted = true;
                started = true;
            }
            '"' => return Err("INVALID_CSV_QUOTES"),
            ',' => {
                row.push(std::mem::take(&mut field));
                if row.len() >= 128 {
                    return Err("TOO_MANY_REPORT_COLUMNS");
                }
                closed = false;
                started = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                if rows.len() > 100_001 {
                    return Err("TOO_MANY_REPORT_LINES");
                }
                closed = false;
                started = false;
            }
            _ if closed => return Err("INVALID_CSV_QUOTES"),
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err("INVALID_CSV_QUOTES");
    }
    if started || closed || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    if rows.len() > 100_001 {
        return Err("TOO_MANY_REPORT_LINES");
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::records;

    #[test]
    fn quoted_fields_and_embedded_records_are_preserved() {
        let rows =
            records("\u{feff}a,b,c\r\n\"comma,value\",\"a\"\"b\",\"two\r\nlines\"\r\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], ["comma,value", "a\"b", "two\r\nlines"]);
    }

    #[test]
    fn malformed_quotes_are_not_accepted() {
        for input in ["a\n\"unfinished", "a\na\"b", "a\n\"x\"garbage"] {
            assert!(records(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn final_empty_field_and_no_final_newline() {
        assert_eq!(records("a,b\nvalue,").unwrap()[1], ["value", ""]);
        assert_eq!(records("a\n\"\"").unwrap()[1], [""]);
        assert!(records("").unwrap().is_empty());
    }

    #[test]
    fn record_limit_also_applies_without_final_newline() {
        let input = format!("{}x", "x\n".repeat(100_001));
        assert!(records(&input).is_err());
    }

    #[test]
    fn column_limit_bounds_many_empty_fields() {
        assert!(records(&",".repeat(128)).is_err());
        assert_eq!(records(&",".repeat(127)).unwrap()[0].len(), 128);
    }
}
