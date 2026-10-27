//! Reading and writing files: boundaries (Shapefile, GeoJSON) and tables
//! (CSV, Excel).

pub mod export;
pub mod files;
pub mod geocode;
pub mod gpkg;
pub mod http;
pub mod kml;
pub mod online;
pub mod routing;
pub mod table;
pub mod vector;
pub mod vectortiles;

pub use table::{TABLE_EXTENSIONS, TableInfo, TableOptions, read_table};
pub use vector::{
    Geometries, GeometryKind, ImportedLayer, VECTOR_EXTENSIONS, VectorOptions, read_vector,
};

/// The API key of an online service. Never written to the settings file
/// (the app keeps it in the system keychain) nor to logs.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ApiKey(pub String);

impl ApiKey {
    pub fn get(&self) -> Option<&str> {
        Some(self.0.trim()).filter(|k| !k.is_empty())
    }

    /// `url` with the key in place of `{key}`; without a placeholder, as a
    /// `key` parameter when `append` (nothing without a key).
    pub fn apply(&self, url: &str, append: bool) -> String {
        let key = self.get().map(encode).unwrap_or_default();
        if url.contains("{key}") {
            url.replace("{key}", &key)
        } else if append && !key.is_empty() {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}key={key}")
        } else {
            url.to_owned()
        }
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.get().is_some() {
            "ApiKey(…)"
        } else {
            "ApiKey(none)"
        })
    }
}

/// Percent-encoding of everything but unreserved URL characters.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0} is missing")]
    Missing(String),
    #[error("files of type `{0}` can't be read")]
    Unsupported(String),
    #[error("{0}")]
    Format(String),
}

#[cfg(test)]
mod tests {
    use super::ApiKey;

    #[test]
    fn keys_go_in_place_or_as_a_parameter() {
        let k = ApiKey("a b/c".into());
        assert_eq!(
            k.apply("https://maps.example/{key}/tiles/{z}", false),
            "https://maps.example/a%20b%2Fc/tiles/{z}"
        );
        assert_eq!(
            k.apply("https://route.example/table?x=1", true),
            "https://route.example/table?x=1&key=a%20b%2Fc"
        );
        assert_eq!(
            k.apply("https://tile.example/{z}", false),
            "https://tile.example/{z}"
        );
        let none = ApiKey::default();
        assert_eq!(
            none.apply("https://r.example/t", true),
            "https://r.example/t"
        );
        assert_eq!(
            none.apply("https://r.example/{key}/t", true),
            "https://r.example//t"
        );
        assert_eq!(format!("{k:?}"), "ApiKey(…)", "never in logs");
    }
}
