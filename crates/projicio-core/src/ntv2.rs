use crate::Error;
use std::io::Read;

/// NTv2 grid shift file reader.
///
/// NTv2 (National Transformation version 2) stores lat/lon shifts on a regular
/// grid for high-accuracy datum transformations. Used by Canada (NAD27→NAD83),
/// Australia (AGD66/84→GDA94), UK (OSGB36→ETRS89), France (NTF→RGF93), etc.
///
/// File format: binary with big-endian or little-endian records.
/// Each sub-grid contains shift values (in arc-seconds) at regular intervals.
///
/// This is a standalone reader for inspecting a grid directly. It is not what
/// [`crate::Transform`] uses: to make a transform apply a grid, register the file with
/// [`crate::grids::register_file`].
#[derive(Debug, Clone)]
pub struct NTv2Grid {
    pub sub_grids: Vec<SubGrid>,
}

/// A single sub-grid within an NTv2 file.
#[derive(Debug, Clone)]
pub struct SubGrid {
    pub name: String,
    pub parent: String,
    /// Southern latitude limit (radians)
    pub lat_min: f64,
    /// Northern latitude limit (radians)
    pub lat_max: f64,
    /// Western longitude limit (radians) — positive west in NTv2!
    pub lon_min: f64,
    /// Eastern longitude limit (radians)
    pub lon_max: f64,
    /// Latitude increment (radians)
    pub lat_inc: f64,
    /// Longitude increment (radians)
    pub lon_inc: f64,
    /// Number of rows (latitude)
    pub n_rows: usize,
    /// Number of columns (longitude)
    pub n_cols: usize,
    /// Shift values: (lat_shift_seconds, lon_shift_seconds) row-major, south to north
    pub shifts: Vec<(f64, f64)>,
}

impl NTv2Grid {
    /// Parse an NTv2 grid file from a byte reader.
    ///
    /// Supports both little-endian (most common) and big-endian variants.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|_| Error::UnsupportedCrs("failed to read NTv2 file".into()))?;

        if buf.len() < 176 {
            return Err(Error::UnsupportedCrs("NTv2 file too small".into()));
        }

        // Detect endianness from NUM_OREC field at offset 8
        // It should be 11 for the overview header
        let le = i32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let be = i32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let is_le = (le == 11) || (le.abs() - 11).abs() < (be.abs() - 11).abs();

        let read_i32 = if is_le {
            |b: &[u8]| i32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            |b: &[u8]| i32::from_be_bytes([b[0], b[1], b[2], b[3]])
        };

        let read_f64 = if is_le {
            |b: &[u8]| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        } else {
            |b: &[u8]| f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        };

        // Overview header: 11 records × 16 bytes = 176 bytes
        let num_sub = read_i32(&buf[40..44]) as usize;

        let mut sub_grids = Vec::with_capacity(num_sub);
        let mut offset = 176; // Skip overview header

        for _ in 0..num_sub {
            if offset + 176 > buf.len() {
                break;
            }

            // Sub-grid header: 11 records × 16 bytes
            let name = read_string(&buf[offset + 8..offset + 16]);
            let parent = read_string(&buf[offset + 24..offset + 32]);

            let lat_min = read_f64(&buf[offset + 72..offset + 80]);
            let lat_max = read_f64(&buf[offset + 88..offset + 96]);
            let lon_min = read_f64(&buf[offset + 104..offset + 112]);
            let lon_max = read_f64(&buf[offset + 120..offset + 128]);
            let lat_inc = read_f64(&buf[offset + 136..offset + 144]);
            let lon_inc = read_f64(&buf[offset + 152..offset + 160]);
            let gs_count = read_i32(&buf[offset + 168..offset + 172]) as usize;

            let n_rows = ((lat_max - lat_min) / lat_inc).round() as usize + 1;
            let n_cols = ((lon_max - lon_min) / lon_inc).round() as usize + 1;

            offset += 176; // Past sub-grid header

            // Read shift records: each is 16 bytes (4×f32)
            let mut shifts = Vec::with_capacity(gs_count);
            let record_size = 16;

            for i in 0..gs_count {
                let rec_offset = offset + i * record_size;
                if rec_offset + record_size > buf.len() {
                    break;
                }
                let lat_shift = if is_le {
                    f32::from_le_bytes([
                        buf[rec_offset],
                        buf[rec_offset + 1],
                        buf[rec_offset + 2],
                        buf[rec_offset + 3],
                    ])
                } else {
                    f32::from_be_bytes([
                        buf[rec_offset],
                        buf[rec_offset + 1],
                        buf[rec_offset + 2],
                        buf[rec_offset + 3],
                    ])
                } as f64;

                let lon_shift = if is_le {
                    f32::from_le_bytes([
                        buf[rec_offset + 4],
                        buf[rec_offset + 5],
                        buf[rec_offset + 6],
                        buf[rec_offset + 7],
                    ])
                } else {
                    f32::from_be_bytes([
                        buf[rec_offset + 4],
                        buf[rec_offset + 5],
                        buf[rec_offset + 6],
                        buf[rec_offset + 7],
                    ])
                } as f64;

                shifts.push((lat_shift, lon_shift));
            }

            offset += gs_count * record_size;

            // Convert arc-seconds to radians for limits
            let as_to_rad = std::f64::consts::PI / (180.0 * 3600.0);

            sub_grids.push(SubGrid {
                name,
                parent,
                lat_min: lat_min * as_to_rad,
                lat_max: lat_max * as_to_rad,
                lon_min: lon_min * as_to_rad,
                lon_max: lon_max * as_to_rad,
                lat_inc: lat_inc * as_to_rad,
                lon_inc: lon_inc * as_to_rad,
                n_rows,
                n_cols,
                shifts,
            });
        }

        Ok(Self { sub_grids })
    }

    /// Apply the grid shift to transform a geographic coordinate.
    ///
    /// Input: (latitude_radians, longitude_radians) in source datum.
    /// NTv2 convention: longitude is positive WEST.
    ///
    /// Returns: (shifted_lat, shifted_lon) in target datum, or None if outside grid.
    pub fn forward(&self, lat: f64, lon: f64) -> Option<(f64, f64)> {
        // NTv2 uses positive-west longitude
        let lon_pos_west = -lon;

        // Find the most detailed sub-grid containing this point
        let grid = self.find_grid(lat, lon_pos_west)?;

        // Bilinear interpolation of shift values
        let (lat_shift, lon_shift) = grid.interpolate(lat, lon_pos_west)?;

        let as_to_rad = std::f64::consts::PI / (180.0 * 3600.0);
        let new_lat = lat + lat_shift * as_to_rad;
        let new_lon = lon - lon_shift * as_to_rad; // Subtract because NTv2 lon shift is positive-west

        Some((new_lat, new_lon))
    }

    /// Find the most specific sub-grid containing the point.
    fn find_grid(&self, lat: f64, lon_pos_west: f64) -> Option<&SubGrid> {
        // Prefer child grids (more detailed) over parent grids
        let mut best: Option<&SubGrid> = None;
        for grid in &self.sub_grids {
            if lat >= grid.lat_min
                && lat <= grid.lat_max
                && lon_pos_west >= grid.lon_min
                && lon_pos_west <= grid.lon_max
            {
                match best {
                    None => best = Some(grid),
                    Some(prev) => {
                        // Prefer smaller (more detailed) grids
                        let prev_area =
                            (prev.lat_max - prev.lat_min) * (prev.lon_max - prev.lon_min);
                        let this_area =
                            (grid.lat_max - grid.lat_min) * (grid.lon_max - grid.lon_min);
                        if this_area < prev_area {
                            best = Some(grid);
                        }
                    }
                }
            }
        }
        best
    }
}

impl SubGrid {
    /// Bilinear interpolation of shift values at the given position.
    ///
    /// Returns (lat_shift_arcsec, lon_shift_arcsec).
    pub fn interpolate(&self, lat: f64, lon_pos_west: f64) -> Option<(f64, f64)> {
        if self.lat_inc == 0.0 || self.lon_inc == 0.0 {
            return None;
        }

        // Fractional grid indices
        let row_f = (lat - self.lat_min) / self.lat_inc;
        let col_f = (lon_pos_west - self.lon_min) / self.lon_inc;

        let row = row_f.floor() as i64;
        let col = col_f.floor() as i64;

        if row < 0 || col < 0 {
            return None;
        }

        let row = row as usize;
        let col = col as usize;

        if row + 1 >= self.n_rows || col + 1 >= self.n_cols {
            return None;
        }

        // Fractional parts
        let t = row_f - row as f64;
        let u = col_f - col as f64;

        // Get four corner shift values
        let sw = self.get_shift(row, col)?;
        let se = self.get_shift(row, col + 1)?;
        let nw = self.get_shift(row + 1, col)?;
        let ne = self.get_shift(row + 1, col + 1)?;

        // Bilinear interpolation
        let lat_shift = (1.0 - t) * (1.0 - u) * sw.0
            + (1.0 - t) * u * se.0
            + t * (1.0 - u) * nw.0
            + t * u * ne.0;

        let lon_shift = (1.0 - t) * (1.0 - u) * sw.1
            + (1.0 - t) * u * se.1
            + t * (1.0 - u) * nw.1
            + t * u * ne.1;

        Some((lat_shift, lon_shift))
    }

    fn get_shift(&self, row: usize, col: usize) -> Option<(f64, f64)> {
        let idx = row * self.n_cols + col;
        self.shifts.get(idx).copied()
    }
}

fn read_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subgrid_interpolation() {
        // Create a simple 2x2 sub-grid with known shifts
        let as_to_rad = std::f64::consts::PI / (180.0 * 3600.0);
        let grid = SubGrid {
            name: "TEST".to_string(),
            parent: "NONE".to_string(),
            lat_min: 45.0_f64.to_radians(),
            lat_max: 46.0_f64.to_radians(),
            lon_min: 70.0_f64.to_radians(), // positive west
            lon_max: 71.0_f64.to_radians(),
            lat_inc: 1.0_f64.to_radians(),
            lon_inc: 1.0_f64.to_radians(),
            n_rows: 2,
            n_cols: 2,
            shifts: vec![
                (1.0, 2.0), // SW corner: 1" lat, 2" lon
                (1.0, 2.0), // SE corner
                (1.0, 2.0), // NW corner
                (1.0, 2.0), // NE corner
            ],
        };

        // Interpolate at center of grid
        let lat = 45.5_f64.to_radians();
        let lon = 70.5_f64.to_radians();
        let (lat_shift, lon_shift) = grid.interpolate(lat, lon).unwrap();

        // With uniform shifts, result should be exactly (1.0, 2.0)
        assert!((lat_shift - 1.0).abs() < 1e-10);
        assert!((lon_shift - 2.0).abs() < 1e-10);

        // Verify the shift magnitude is reasonable (arc-seconds)
        let lat_shift_meters = lat_shift * as_to_rad * 6_371_000.0;
        assert!(
            lat_shift_meters.abs() < 100.0,
            "1 arcsec ≈ 31m, got {lat_shift_meters}"
        );
    }

    #[test]
    fn test_subgrid_bilinear_interpolation() {
        // 3x3 grid with varying shifts
        let grid = SubGrid {
            name: "TEST".to_string(),
            parent: "NONE".to_string(),
            lat_min: 0.0,
            lat_max: 2.0,
            lon_min: 0.0,
            lon_max: 2.0,
            lat_inc: 1.0,
            lon_inc: 1.0,
            n_rows: 3,
            n_cols: 3,
            shifts: vec![
                // row 0 (south)
                (0.0, 0.0),
                (1.0, 0.0),
                (2.0, 0.0),
                // row 1 (middle)
                (0.0, 1.0),
                (1.0, 1.0),
                (2.0, 1.0),
                // row 2 (north)
                (0.0, 2.0),
                (1.0, 2.0),
                (2.0, 2.0),
            ],
        };

        // At grid node (1, 1): should be exactly (1.0, 1.0)
        let (ls, lo) = grid.interpolate(1.0, 1.0).unwrap();
        assert!((ls - 1.0).abs() < 1e-10);
        assert!((lo - 1.0).abs() < 1e-10);

        // At midpoint (0.5, 0.5): bilinear of (0,0),(1,0),(0,1),(1,1) = (0.5, 0.5)
        let (ls, lo) = grid.interpolate(0.5, 0.5).unwrap();
        assert!((ls - 0.5).abs() < 1e-10);
        assert!((lo - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_ntv2_too_small() {
        let data = vec![0u8; 100];
        let mut cursor = std::io::Cursor::new(data);
        let result = NTv2Grid::read(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_grid_prefers_detailed() {
        let ntv2 = NTv2Grid {
            sub_grids: vec![
                SubGrid {
                    name: "LARGE".to_string(),
                    parent: "NONE".to_string(),
                    lat_min: 0.0,
                    lat_max: 10.0,
                    lon_min: 0.0,
                    lon_max: 10.0,
                    lat_inc: 1.0,
                    lon_inc: 1.0,
                    n_rows: 11,
                    n_cols: 11,
                    shifts: vec![(1.0, 1.0); 121],
                },
                SubGrid {
                    name: "SMALL".to_string(),
                    parent: "LARGE".to_string(),
                    lat_min: 4.0,
                    lat_max: 6.0,
                    lon_min: 4.0,
                    lon_max: 6.0,
                    lat_inc: 1.0,
                    lon_inc: 1.0,
                    n_rows: 3,
                    n_cols: 3,
                    shifts: vec![(2.0, 2.0); 9],
                },
            ],
        };

        let grid = ntv2.find_grid(5.0, 5.0).unwrap();
        assert_eq!(grid.name, "SMALL");
    }
}
