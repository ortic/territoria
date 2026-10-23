//! Reading and writing files: boundaries (Shapefile, GeoJSON) and tables
//! (CSV, Excel).

pub mod export;
pub mod geocode;
pub mod gpkg;
pub mod online;
pub mod table;
pub mod vector;

pub use table::{TABLE_EXTENSIONS, TableInfo, TableOptions, read_table};
pub use vector::{
    Geometries, GeometryKind, ImportedLayer, VECTOR_EXTENSIONS, VectorOptions, read_vector,
};

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
