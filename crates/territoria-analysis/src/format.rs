//! Numbers for people: thousands separators and sensible decimals.

/// `1234567` → `1,234,567`; `0.03456` → `0.035`; `12.5` → `12.5`.
pub fn format_value(v: f64) -> String {
    if !v.is_finite() {
        return "–".into();
    }
    let abs = v.abs();
    let decimals = if abs == 0.0 || abs >= 100.0 {
        0
    } else if abs >= 10.0 {
        1
    } else if abs >= 1.0 {
        2
    } else {
        // Two significant digits below one.
        (2 - abs.log10().floor() as i32 - 1).clamp(2, 8) as usize
    };
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    group_thousands(&s)
}

/// Inserts `,` every three digits of the integer part.
pub fn group_thousands(s: &str) -> String {
    let (sign, rest) = s.strip_prefix('-').map_or(("", s), |r| ("-", r));
    let (int, frac) = rest
        .split_once('.')
        .map_or((rest, None), |(a, b)| (a, Some(b)));
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

/// Percent with one decimal, `0.1234` → `12.3 %`.
pub fn format_percent(share: f64) -> String {
    if !share.is_finite() {
        return "–".into();
    }
    format!("{:.1} %", share * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_value(1_234_567.0), "1,234,567");
        assert_eq!(format_value(-1234.4), "-1,234");
        assert_eq!(format_value(12.5), "12.5");
        assert_eq!(format_value(3.14659), "3.15");
        assert_eq!(format_value(0.03456), "0.035");
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(100.0), "100");
        assert_eq!(format_percent(0.1234), "12.3 %");
    }
}
