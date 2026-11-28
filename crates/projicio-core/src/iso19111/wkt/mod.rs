mod database;
mod formatter;
mod mappings;
mod methods;
mod node;
mod other_method;
mod reader;
mod writer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WktDialect {
    Wkt2_2019,
    Wkt2_2015,
    Wkt1Gdal,
    Wkt1Esri,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WktLayout {
    MultiLine,
    SingleLine,
}
