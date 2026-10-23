//! Calendar dates as days since 1970-01-01 (proleptic Gregorian).

/// Days since 1970-01-01 for a calendar date (H. Hinnant's algorithm).
pub fn from_ymd(y: i32, m: u32, d: u32) -> Option<i32> {
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

pub fn to_ymd(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// ISO format, `2026-10-23`.
pub fn format(days: i32) -> String {
    let (y, m, d) = to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Excel's serial day number (1900 date system) to days since 1970.
pub fn from_excel(serial: f64) -> i32 {
    (serial.floor() as i32) - 25_569
}

/// Day order of numeric dates like `03/04/2026`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayOrder {
    /// 2026-10-23, 23.10.2026, 23/10/2026
    DayMonth,
    /// 10/23/2026 (US)
    MonthDay,
}

/// Parses `2026-10-23`, `2026-10-23T12:00…`, `23.10.2026`, `23/10/2026` or
/// `10/23/2026` (by `order`), with two-digit years read as 19xx/20xx.
pub fn parse(text: &str, order: DayOrder) -> Option<i32> {
    let t = text.trim();
    let t = t.split(['T', ' ']).next().unwrap_or(t);
    let (sep, iso) = if t.len() >= 8 && t.as_bytes().get(4) == Some(&b'-') {
        ('-', true)
    } else if t.contains('.') {
        ('.', false)
    } else if t.contains('/') {
        ('/', false)
    } else if t.contains('-') {
        ('-', false)
    } else {
        return None;
    };
    let parts: Vec<&str> = t.split(sep).collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let n: Vec<i64> = parts.iter().map(|p| p.parse().unwrap_or(-1)).collect();
    let (y, m, d) = if iso {
        (n[0], n[1], n[2])
    } else if sep == '/' && order == DayOrder::MonthDay {
        (n[2], n[0], n[1])
    } else {
        (n[2], n[1], n[0])
    };
    let y = match (parts[if iso { 0 } else { 2 }].len(), y) {
        (2, y) if y < 70 => 2000 + y,
        (2, y) => 1900 + y,
        (4, y) => y,
        _ => return None,
    };
    from_ymd(y as i32, u32::try_from(m).ok()?, u32::try_from(d).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(from_ymd(1970, 1, 1), Some(0));
        assert_eq!(from_ymd(2000, 3, 1), Some(11_017));
        for days in [-800_000, -1, 0, 59, 11_016, 20_000, 2_000_000] {
            let (y, m, d) = to_ymd(days);
            assert_eq!(from_ymd(y, m, d), Some(days));
        }
        assert_eq!(from_ymd(2025, 2, 29), None);
        assert_eq!(format(from_ymd(2026, 10, 23).unwrap()), "2026-10-23");
    }

    #[test]
    fn parses_common_formats() {
        let want = from_ymd(2026, 10, 3);
        assert_eq!(parse("2026-10-03", DayOrder::DayMonth), want);
        assert_eq!(parse("2026-10-03T08:00:00Z", DayOrder::DayMonth), want);
        assert_eq!(parse("03.10.2026", DayOrder::DayMonth), want);
        assert_eq!(parse("3.10.26", DayOrder::DayMonth), want);
        assert_eq!(parse("03/10/2026", DayOrder::DayMonth), want);
        assert_eq!(parse("10/03/2026", DayOrder::MonthDay), want);
        assert_eq!(parse("10/3", DayOrder::DayMonth), None);
        assert_eq!(parse("8001", DayOrder::DayMonth), None);
    }

    #[test]
    fn excel_serials() {
        assert_eq!(from_excel(25_569.0), 0);
        assert_eq!(format(from_excel(46_318.5)), "2026-10-23");
    }
}
