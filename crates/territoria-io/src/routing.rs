//! Travel by road from an OSRM server (the public demo for trying, or
//! your own): time and distance from many places to the nearest of a
//! few others, asked in blocks of the matrix the server accepts.

use std::time::{Duration, Instant};

use geo_types::Coord;
use serde::{Deserialize, Serialize};

use crate::ImportError;

/// Which server, which way of travelling, and how politely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingSettings {
    pub url: String,
    /// `driving`, `cycling`, `walking` (what the server was built with).
    pub profile: String,
    /// Places per request (origins and destinations together).
    pub batch: usize,
    /// Pause between requests in milliseconds.
    pub interval_ms: u64,
}

impl Default for RoutingSettings {
    fn default() -> Self {
        RoutingSettings {
            url: "https://router.project-osrm.org".into(),
            profile: "driving".into(),
            batch: 100,
            interval_ms: 1000,
        }
    }
}

/// The nearest destination by road of an origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    pub destination: usize,
    pub minutes: f64,
    pub km: f64,
}

#[derive(Deserialize)]
struct TableResponse {
    code: String,
    #[serde(default)]
    message: Option<String>,
    durations: Option<Vec<Vec<Option<f64>>>>,
    distances: Option<Vec<Vec<Option<f64>>>>,
}

/// For each origin (longitude/latitude) the destination reached fastest,
/// with minutes and km; `None` where none can be reached. `progress` gets
/// (done, total) requests and stops the work by returning false.
pub fn nearest_by_road(
    settings: &RoutingSettings,
    origins: &[Coord<f64>],
    destinations: &[Coord<f64>],
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Vec<Option<Nearest>>, ImportError> {
    let mut best: Vec<Option<Nearest>> = vec![None; origins.len()];
    if origins.is_empty() || destinations.is_empty() {
        return Ok(best);
    }
    let batch = settings.batch.max(2);
    // Destinations take at most half of each request.
    let per_dest = destinations.len().min(batch / 2).max(1);
    let per_origin = (batch - per_dest).max(1);
    let total = destinations.len().div_ceil(per_dest) * origins.len().div_ceil(per_origin);
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("territoria/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();
    let base = settings.url.trim_end_matches('/');
    let mut done = 0;
    let mut last: Option<Instant> = None;
    for (dc, dests) in destinations.chunks(per_dest).enumerate() {
        for (oc, origs) in origins.chunks(per_origin).enumerate() {
            if !progress(done, total) {
                return Err(ImportError::Format("stopped".into()));
            }
            if let Some(t) = last {
                let wait = Duration::from_millis(settings.interval_ms).saturating_sub(t.elapsed());
                std::thread::sleep(wait);
            }
            last = Some(Instant::now());
            let coords: Vec<String> = origs
                .iter()
                .chain(dests)
                .map(|c| format!("{:.6},{:.6}", c.x, c.y))
                .collect();
            let sources: Vec<String> = (0..origs.len()).map(|i| i.to_string()).collect();
            let targets: Vec<String> = (origs.len()..origs.len() + dests.len())
                .map(|i| i.to_string())
                .collect();
            let url = format!(
                "{base}/table/v1/{}/{}?sources={}&destinations={}&annotations=duration,distance",
                settings.profile,
                coords.join(";"),
                sources.join(";"),
                targets.join(";")
            );
            let text = agent
                .get(&url)
                .call()
                .map_err(|e| ImportError::Format(format!("routing: {e}")))?
                .body_mut()
                .read_to_string()
                .map_err(|e| ImportError::Format(format!("routing: {e}")))?;
            let r: TableResponse = serde_json::from_str(&text)
                .map_err(|e| ImportError::Format(format!("routing: unexpected answer ({e})")))?;
            if r.code != "Ok" {
                return Err(ImportError::Format(format!(
                    "routing: {}",
                    r.message.unwrap_or(r.code)
                )));
            }
            let durations = r.durations.unwrap_or_default();
            let distances = r.distances.unwrap_or_default();
            for (i, row) in durations.iter().enumerate() {
                let o = oc * per_origin + i;
                for (j, d) in row.iter().enumerate() {
                    let Some(seconds) = d else { continue };
                    let minutes = seconds / 60.0;
                    if best[o].is_none_or(|b| minutes < b.minutes) {
                        let metres = distances
                            .get(i)
                            .and_then(|r| r.get(j))
                            .copied()
                            .flatten()
                            .unwrap_or(f64::NAN);
                        best[o] = Some(Nearest {
                            destination: dc * per_dest + j,
                            minutes,
                            km: metres / 1000.0,
                        });
                    }
                }
            }
            done += 1;
        }
    }
    progress(total, total);
    Ok(best)
}

/// A visiting round: the stops in the order to visit them, the road
/// there and back, and its length.
#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    /// Per stop (as given) its place in the round, from 0.
    pub order: Vec<usize>,
    /// The road, longitude/latitude.
    pub path: Vec<Coord<f64>>,
    pub minutes: f64,
    pub km: f64,
}

#[derive(Deserialize)]
struct TripResponse {
    code: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    trips: Vec<TripRoute>,
    #[serde(default)]
    waypoints: Vec<TripWaypoint>,
}

#[derive(Deserialize)]
struct TripRoute {
    geometry: TripGeometry,
    duration: f64,
    distance: f64,
}

#[derive(Deserialize)]
struct TripGeometry {
    coordinates: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct TripWaypoint {
    waypoint_index: usize,
}

/// The quickest round through `stops` (longitude/latitude), starting and
/// ending at the first (OSRM's `trip` service).
pub fn trip(settings: &RoutingSettings, stops: &[Coord<f64>]) -> Result<Trip, ImportError> {
    if stops.len() < 2 {
        return Err(ImportError::Format(
            "a round needs two stops or more".into(),
        ));
    }
    let coords: Vec<String> = stops
        .iter()
        .map(|c| format!("{:.6},{:.6}", c.x, c.y))
        .collect();
    let url = format!(
        "{}/trip/v1/{}/{}?roundtrip=true&source=first&geometries=geojson&overview=full",
        settings.url.trim_end_matches('/'),
        settings.profile,
        coords.join(";")
    );
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(concat!("territoria/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();
    let net = |e: ureq::Error| ImportError::Format(format!("routing: {e}"));
    let text = agent
        .get(&url)
        .call()
        .map_err(net)?
        .body_mut()
        .read_to_string()
        .map_err(net)?;
    let r: TripResponse = serde_json::from_str(&text)
        .map_err(|e| ImportError::Format(format!("routing: unexpected answer ({e})")))?;
    if r.code != "Ok" {
        return Err(ImportError::Format(format!(
            "routing: {}",
            r.message.unwrap_or(r.code)
        )));
    }
    let route = r
        .trips
        .first()
        .ok_or_else(|| ImportError::Format("routing: no round found".into()))?;
    Ok(Trip {
        order: r.waypoints.iter().map(|w| w.waypoint_index).collect(),
        path: route
            .geometry
            .coordinates
            .iter()
            .map(|[x, y]| Coord { x: *x, y: *y })
            .collect(),
        minutes: route.duration / 60.0,
        km: route.distance / 1000.0,
    })
}

/// An OSRM stand-in for tests: a second of travel per 0.001° of straight
/// line, a km per 0.01°. Returns its address.
#[doc(hidden)]
pub fn mock_server() -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or_default();
            let (route, query) = path.split_once('?').unwrap_or((path, ""));
            let coords: Vec<(f64, f64)> = route
                .rsplit('/')
                .next()
                .unwrap()
                .split(';')
                .map(|c| {
                    let (x, y) = c.split_once(',').unwrap();
                    (x.parse().unwrap(), y.parse().unwrap())
                })
                .collect();
            let dist = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
            if route.contains("/trip/") {
                // Nearest neighbour from the first stop, and back.
                let mut order = vec![0usize];
                while order.len() < coords.len() {
                    let last = coords[*order.last().unwrap()];
                    let next = (0..coords.len())
                        .filter(|i| !order.contains(i))
                        .min_by(|a, b| dist(last, coords[*a]).total_cmp(&dist(last, coords[*b])))
                        .unwrap();
                    order.push(next);
                }
                let mut path: Vec<(f64, f64)> = order.iter().map(|&i| coords[i]).collect();
                path.push(coords[0]);
                let length: f64 = path.windows(2).map(|w| dist(w[0], w[1])).sum();
                let mut rank = vec![0; coords.len()];
                for (k, &i) in order.iter().enumerate() {
                    rank[i] = k;
                }
                let points: Vec<String> =
                    path.iter().map(|p| format!("[{},{}]", p.0, p.1)).collect();
                let waypoints: Vec<String> = rank
                    .iter()
                    .map(|k| format!(r#"{{"waypoint_index":{k},"trips_index":0}}"#))
                    .collect();
                let body = format!(
                    r#"{{"code":"Ok","trips":[{{"geometry":{{"type":"LineString","coordinates":[{}]}},"duration":{},"distance":{}}}],"waypoints":[{}]}}"#,
                    points.join(","),
                    length * 1000.0,
                    length * 100_000.0,
                    waypoints.join(",")
                );
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                continue;
            }
            let list = |name: &str| -> Vec<usize> {
                query
                    .split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
                    .unwrap()
                    .split(';')
                    .map(|i| i.parse().unwrap())
                    .collect()
            };
            let (sources, targets) = (list("sources"), list("destinations"));
            let matrix = |scale: f64| -> String {
                let rows: Vec<String> = sources
                    .iter()
                    .map(|&s| {
                        let cells: Vec<String> = targets
                            .iter()
                            .map(|&t| {
                                let (a, b) = (coords[s], coords[t]);
                                format!("{:.1}", (a.0 - b.0).hypot(a.1 - b.1) * scale)
                            })
                            .collect();
                        format!("[{}]", cells.join(","))
                    })
                    .collect();
                format!("[{}]", rows.join(","))
            };
            let body = format!(
                r#"{{"code":"Ok","durations":{},"distances":{}}}"#,
                matrix(1000.0),
                matrix(100_000.0)
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_in_blocks() {
        let settings = RoutingSettings {
            url: mock_server(),
            batch: 4,
            interval_ms: 0,
            ..Default::default()
        };
        let c = |x: f64, y: f64| Coord { x, y };
        let origins = [
            c(8.0, 47.0),
            c(8.1, 47.0),
            c(9.0, 47.5),
            c(7.0, 46.0),
            c(8.95, 47.4),
        ];
        let dests = [c(7.1, 46.1), c(8.05, 47.0), c(9.0, 47.45)];
        let mut calls = 0;
        let found = nearest_by_road(&settings, &origins, &dests, &mut |_, _| {
            calls += 1;
            true
        })
        .unwrap();
        let picked: Vec<usize> = found.iter().map(|n| n.unwrap().destination).collect();
        assert_eq!(picked, vec![1, 1, 2, 0, 2]);
        let first = found[0].unwrap();
        assert!((first.minutes - 50.0 / 60.0).abs() < 1e-6, "{first:?}");
        assert!((first.km - 5.0).abs() < 1e-6);
        // 2 blocks of destinations × 3 of origins, and the final report.
        assert_eq!(calls, 7);
    }

    #[test]
    fn round_through_stops() {
        let settings = RoutingSettings {
            url: mock_server(),
            ..Default::default()
        };
        let c = |x: f64, y: f64| Coord { x, y };
        let stops = [c(0.0, 0.0), c(2.0, 0.0), c(1.0, 0.0), c(3.0, 0.0)];
        let t = trip(&settings, &stops).unwrap();
        assert_eq!(t.order, vec![0, 2, 1, 3]);
        assert_eq!(t.path.len(), 5);
        assert!((t.km - 600.0).abs() < 1e-6, "{}", t.km);
    }
}
