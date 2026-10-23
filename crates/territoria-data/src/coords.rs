//! Coordinates written in one text: `47.3769, 8.5417` (as maps copy
//! them), `47°22'36.8"N 8°32'30.1"E`, `N 47° 22.613 E 008° 32.502`.

/// Degrees from parts like `47 22 36.8` (degrees, minutes, seconds).
fn degrees(parts: &[f64]) -> Option<f64> {
    match parts {
        [d] => Some(*d),
        [d, m] => Some(d.abs() + m / 60.0),
        [d, m, s] => Some(d.abs() + m / 60.0 + s / 3600.0),
        _ => None,
    }
    .map(|v| {
        if parts.first().is_some_and(|d| *d < 0.0) {
            -v.abs()
        } else {
            v
        }
    })
}

/// The numbers in a text, in order (a comma between digits is a decimal
/// comma only when the text has no other separator use for it).
fn numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let next_digit = chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        if c.is_ascii_digit()
            || (c == '.' && next_digit)
            || (c == '-' && cur.is_empty() && next_digit)
        {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse() {
                out.push(v);
            }
            cur.clear();
        }
    }
    if let Ok(v) = cur.parse() {
        out.push(v);
    }
    out
}

/// Latitude and longitude from one text; `None` when it isn't a pair.
pub fn parse_lat_lon(text: &str) -> Option<(f64, f64)> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let upper = t.to_uppercase();
    let hemi = |c: char| matches!(c, 'N' | 'S' | 'E' | 'W' | 'O');
    let letters: Vec<(usize, char)> = upper.char_indices().filter(|(_, c)| hemi(*c)).collect();
    // Each coordinate's numbers with its hemisphere letter (or none).
    let parts: Vec<(String, Option<char>)> = match letters.as_slice() {
        [a, b] => {
            let prefix = upper.trim_start().starts_with(hemi);
            if prefix {
                // N 47° 22.613 E 008° 32.502
                vec![
                    (upper[a.0 + 1..b.0].to_owned(), Some(a.1)),
                    (upper[b.0 + 1..].to_owned(), Some(b.1)),
                ]
            } else {
                // 47°22'36.8"N 8°32'30.1"E
                vec![
                    (upper[..a.0].to_owned(), Some(a.1)),
                    (upper[a.0 + 1..b.0].to_owned(), Some(b.1)),
                ]
            }
        }
        [] => {
            // Without letters: split at a comma or semicolon, else in the
            // middle of the numbers (47 22 36 8 32 30).
            if let Some((x, y)) = t
                .split_once([',', ';'])
                .filter(|(x, y)| !numbers(x).is_empty() && !numbers(y).is_empty())
            {
                vec![(x.to_owned(), None), (y.to_owned(), None)]
            } else {
                let n = numbers(t);
                if !n.len().is_multiple_of(2) || n.is_empty() || n.len() > 6 {
                    return None;
                }
                let half = n.len() / 2;
                let join = |v: &[f64]| {
                    v.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                vec![(join(&n[..half]), None), (join(&n[half..]), None)]
            }
        }
        _ => return None,
    };
    let (mut lat, mut lon) = (None, None);
    for (i, (part, letter)) in parts.iter().enumerate() {
        let v = degrees(&numbers(part))?;
        match letter {
            Some('N') => lat = Some(v),
            Some('S') => lat = Some(-v.abs()),
            Some('E' | 'O') => lon = Some(v),
            Some('W') => lon = Some(-v.abs()),
            _ if i == 0 => lat = Some(v),
            _ => lon = Some(v),
        }
    }
    let (lat, lon) = (lat?, lon?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

/// Whether most texts of a column are coordinate pairs.
pub fn looks_like_lat_lon(texts: &[Option<String>]) -> bool {
    let present: Vec<&str> = texts
        .iter()
        .flatten()
        .map(String::as_str)
        .take(200)
        .collect();
    !present.is_empty()
        && present
            .iter()
            .filter(|t| parse_lat_lon(t).is_some())
            .count()
            * 10
            >= present.len() * 9
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn reads_common_forms() {
        assert_eq!(parse_lat_lon("47.3769, 8.5417"), Some((47.3769, 8.5417)));
        assert_eq!(
            parse_lat_lon("-33.8688 151.2093"),
            Some((-33.8688, 151.2093))
        );
        let dms = parse_lat_lon("47°22'36.8\"N 8°32'30.1\"E").unwrap();
        assert!(close(dms, (47.376_889, 8.541_694)), "{dms:?}");
        let gps = parse_lat_lon("N 47° 22.613 E 008° 32.502").unwrap();
        assert!(close(gps, (47.376_883, 8.5417)), "{gps:?}");
        let west = parse_lat_lon("40°26'46\"N 79°58'56\"W").unwrap();
        assert!(west.1 < -79.9, "{west:?}");
        assert_eq!(parse_lat_lon("Zürich"), None);
        assert_eq!(parse_lat_lon("123, 456"), None, "out of range");
        assert!(looks_like_lat_lon(&[
            Some("47.1, 8.2".into()),
            Some("46.9, 7.4".into()),
            None
        ]));
    }
}
