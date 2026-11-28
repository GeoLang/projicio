// adapted from PROJ src/grids.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::collections::HashMap;
use std::f64::consts::PI;

use super::grid::{Extent, HorizontalShiftGrid, HorizontalShiftGridSet};

const ARC_SECOND_TO_RADIAN: f64 = (PI / 180.0) / 3600.0;

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], String> {
    bytes
        .get(offset..offset + N)
        .and_then(|field| field.try_into().ok())
        .ok_or_else(|| {
            format!(
                "file ends at byte {} before byte {}",
                bytes.len(),
                offset + N
            )
        })
}

fn f64_at(bytes: &[u8], offset: usize, order: ByteOrder) -> Result<f64, String> {
    let field = field(bytes, offset)?;
    Ok(match order {
        ByteOrder::Little => f64::from_le_bytes(field),
        ByteOrder::Big => f64::from_be_bytes(field),
    })
}

fn f32_at(bytes: &[u8], offset: usize, order: ByteOrder) -> Result<f32, String> {
    let field = field(bytes, offset)?;
    Ok(match order {
        ByteOrder::Little => f32::from_le_bytes(field),
        ByteOrder::Big => f32::from_be_bytes(field),
    })
}

fn i32_at(bytes: &[u8], offset: usize, order: ByteOrder) -> Result<i32, String> {
    let field = field(bytes, offset)?;
    Ok(match order {
        ByteOrder::Little => i32::from_le_bytes(field),
        ByteOrder::Big => i32::from_be_bytes(field),
    })
}

fn starts_with_at(bytes: &[u8], offset: usize, text: &[u8]) -> bool {
    bytes.get(offset..offset + text.len()) == Some(text)
}

fn is_tiff(bytes: &[u8]) -> bool {
    // grids.cpp IsTIFF
    let Some(header) = bytes.get(..4) else {
        return false;
    };
    let byte_order_mark = &header[..2] == b"II" || &header[..2] == b"MM";
    let version = [header[2], header[3]];
    byte_order_mark && [[0x2A, 0], [0, 0x2A], [0x2B, 0], [0, 0x2B]].contains(&version)
}

pub(super) fn read_horizontal_shift_grids(
    name: &str,
    bytes: &[u8],
) -> Result<HorizontalShiftGridSet, String> {
    // grids.cpp HorizontalShiftGridSet::open picks the format from the first 160 bytes
    let grid = if bytes.len() >= 160
        && starts_with_at(bytes, 0, b"HEADER")
        && starts_with_at(bytes, 96, b"W GRID")
        && starts_with_at(bytes, 144, b"TO      NAD83   ")
    {
        read_ntv1(bytes)?
    } else if starts_with_at(bytes, 0, b"CTABLE V2") {
        read_ctable2(bytes)?
    } else if starts_with_at(bytes, 0, b"NUM_OREC") && starts_with_at(bytes, 48, b"GS_TYPE") {
        return read_ntv2(name, bytes);
    } else if is_tiff(bytes) {
        // TODO: read GeoTIFF grids through terrano-core once its reader returns each IFD's GDAL_METADATA, tiepoint and NewSubfileType
        return Err("it is a GeoTIFF, and projicio reads no GeoTIFF grids yet".into());
    } else {
        return Err("it is not an NTv1, NTv2 or CTable2 grid".into());
    };
    Ok(HorizontalShiftGridSet {
        name: name.to_string(),
        grids: vec![grid],
        top_level_indices: vec![0],
    })
}

fn checked_extent(extent: Extent) -> Result<Extent, String> {
    let consistent = extent.west.abs() <= 4.0 * PI
        && extent.east.abs() <= 4.0 * PI
        && extent.north.abs() <= PI + 1e-5
        && extent.south.abs() <= PI + 1e-5
        && extent.west < extent.east
        && extent.south < extent.north
        && extent.resolution_x > 1e-10
        && extent.resolution_y > 1e-10;
    if !consistent {
        return Err("inconsistent georeferencing".into());
    }
    Ok(extent)
}

fn node_count(extent: &Extent) -> (usize, usize) {
    // grids.cpp rounds with fabs(span * invRes + 0.5) + 1 and truncates to int
    let count =
        |span: f64, resolution: f64| ((span * (1.0 / resolution) + 0.5).abs() + 1.0) as usize;
    (
        count(extent.east - extent.west, extent.resolution_x),
        count(extent.north - extent.south, extent.resolution_y),
    )
}

fn check_grid_size(
    bytes: &[u8],
    start: usize,
    width: usize,
    height: usize,
    node_size: usize,
) -> Result<(), String> {
    if width < 2 || height < 2 {
        return Err(format!(
            "a {width} by {height} grid has no cell to interpolate in"
        ));
    }
    let end = width
        .checked_mul(height)
        .and_then(|nodes| nodes.checked_mul(node_size))
        .and_then(|size| size.checked_add(start))
        .ok_or_else(|| format!("a {width} by {height} grid does not fit in memory"))?;
    if end > bytes.len() {
        return Err(format!(
            "file ends at byte {} and the {width} by {height} grid needs {end}",
            bytes.len()
        ));
    }
    Ok(())
}

fn read_ntv1(bytes: &[u8]) -> Result<HorizontalShiftGrid, String> {
    // grids.cpp NTv1Grid::open and NTv1Grid::valueAt
    const HEADER_SIZE: usize = 192;
    let order = ByteOrder::Big;
    if i32_at(bytes, 8, order)? != 12 {
        return Err("NTv1 grid shift file has wrong record count, corrupt?".into());
    }
    let radians_from_degrees = |offset| f64_at(bytes, offset, order).map(f64::to_radians);
    // ntv1 counts longitude positive west
    let extent = checked_extent(Extent {
        west: -radians_from_degrees(72)?,
        south: radians_from_degrees(24)?,
        east: -radians_from_degrees(56)?,
        north: radians_from_degrees(40)?,
        resolution_x: radians_from_degrees(104)?,
        resolution_y: radians_from_degrees(88)?,
    })?;
    let (width, height) = node_count(&extent);
    check_grid_size(bytes, HEADER_SIZE, width, height, 16)?;
    let mut shifts = Vec::with_capacity(width * height);
    for row in 0..height {
        for column in 0..width {
            // ntv1 stores each row east to west
            let offset = HEADER_SIZE + 16 * (row * width + width - 1 - column);
            let latitude = f64_at(bytes, offset, order)?;
            let longitude_west = f64_at(bytes, offset + 8, order)?;
            shifts.push([
                -((longitude_west * ARC_SECOND_TO_RADIAN) as f32),
                (latitude * ARC_SECOND_TO_RADIAN) as f32,
            ]);
        }
    }
    Ok(HorizontalShiftGrid {
        width,
        height,
        extent,
        shifts,
        child_indices: Vec::new(),
    })
}

fn read_ctable2(bytes: &[u8]) -> Result<HorizontalShiftGrid, String> {
    // grids.cpp CTable2Grid::open and CTable2Grid::valueAt
    const HEADER_SIZE: usize = 160;
    let order = ByteOrder::Little;
    let west = f64_at(bytes, 96, order)?;
    let south = f64_at(bytes, 104, order)?;
    let resolution_x = f64_at(bytes, 112, order)?;
    let resolution_y = f64_at(bytes, 120, order)?;
    if !(west.abs() <= 4.0 * PI
        && south.abs() <= PI + 1e-5
        && resolution_x > 1e-10
        && resolution_y > 1e-10)
    {
        return Err("inconsistent georeferencing".into());
    }
    let width = i32_at(bytes, 128, order)?;
    let height = i32_at(bytes, 132, order)?;
    if width <= 0 || height <= 0 {
        return Err(format!("grid is {width} by {height} nodes"));
    }
    let (width, height) = (width as usize, height as usize);
    // CTable2Grid::open in PROJ 9.8.1 steps north by the longitude resolution
    let extent = Extent {
        west,
        south,
        east: west + (width - 1) as f64 * resolution_x,
        north: south + (height - 1) as f64 * resolution_x,
        resolution_x,
        resolution_y,
    };
    check_grid_size(bytes, HEADER_SIZE, width, height, 8)?;
    let mut shifts = Vec::with_capacity(width * height);
    for node in 0..width * height {
        let offset = HEADER_SIZE + 8 * node;
        // ctable2 counts longitude positive west
        let longitude_west = f32_at(bytes, offset, order)?;
        let latitude = f32_at(bytes, offset + 4, order)?;
        shifts.push([-longitude_west, latitude]);
    }
    Ok(HorizontalShiftGrid {
        width,
        height,
        extent,
        shifts,
        child_indices: Vec::new(),
    })
}

fn read_ntv2(name: &str, bytes: &[u8]) -> Result<HorizontalShiftGridSet, String> {
    // grids.cpp NTv2GridSet::open and NTv2Grid::valueAt
    const HEADER_SIZE: usize = 11 * 16;
    if bytes.len() < HEADER_SIZE {
        return Err(format!(
            "NTv2 file is {} bytes, shorter than its {HEADER_SIZE} byte overview header",
            bytes.len()
        ));
    }
    if !starts_with_at(bytes, 56, b"SECONDS") {
        return Err("Only GS_TYPE=SECONDS is supported".into());
    }
    let order = if bytes[8] == 11 {
        ByteOrder::Little
    } else {
        ByteOrder::Big
    };
    let subfile_count = i32_at(bytes, 40, order)? as u32;
    if subfile_count == 0 {
        return Err("NTv2 file has no subfiles".into());
    }

    let mut grids: Vec<HorizontalShiftGrid> = Vec::new();
    let mut top_level_indices = Vec::new();
    let mut index_by_name: HashMap<[u8; 8], usize> = HashMap::new();
    let mut offset = HEADER_SIZE;
    for _ in 0..subfile_count {
        if !starts_with_at(bytes, offset, b"SUB_NAME") {
            return Err(format!("no SUB_NAME record at byte {offset}"));
        }
        let subfile_name: [u8; 8] = field(bytes, offset + 8)?;
        let parent_name: [u8; 8] = field(bytes, offset + 24)?;
        let radians_from_arc_seconds = |record: usize| {
            f64_at(bytes, offset + 72 + 16 * record, order).map(|value| value.to_radians() / 3600.0)
        };
        // ntv2 counts longitude positive west
        let extent = checked_extent(Extent {
            south: radians_from_arc_seconds(0)?,
            north: radians_from_arc_seconds(1)?,
            east: -radians_from_arc_seconds(2)?,
            west: -radians_from_arc_seconds(3)?,
            resolution_y: radians_from_arc_seconds(4)?,
            resolution_x: radians_from_arc_seconds(5)?,
        })?;
        let (width, height) = node_count(&extent);
        let node_count_in_file = i32_at(bytes, offset + 168, order)? as u32 as usize;
        if node_count_in_file / width != height {
            return Err(format!(
                "GS_COUNT({node_count_in_file}) does not match expected cells ({width}x{height})"
            ));
        }
        let data = offset + HEADER_SIZE;
        check_grid_size(bytes, data, width, height, 16)?;
        let mut shifts = Vec::with_capacity(width * height);
        for row in 0..height {
            for column in 0..width {
                // ntv2 stores latitude shift, longitude shift and their accuracies, each row east to west
                let node = data + 16 * (row * width + width - 1 - column);
                let latitude = f64::from(f32_at(bytes, node, order)?);
                let longitude_west = f64::from(f32_at(bytes, node + 4, order)?);
                shifts.push([
                    -((longitude_west * ARC_SECOND_TO_RADIAN) as f32),
                    (latitude * ARC_SECOND_TO_RADIAN) as f32,
                ]);
            }
        }
        let index = grids.len();
        match index_by_name.get(&parent_name) {
            Some(&parent) => grids[parent].child_indices.push(index),
            None => top_level_indices.push(index),
        }
        index_by_name.insert(subfile_name, index);
        grids.push(HorizontalShiftGrid {
            width,
            height,
            extent,
            shifts,
            child_indices: Vec::new(),
        });
        // NTv2GridSet::open skips GS_COUNT nodes, which may be more than width times height
        offset = node_count_in_file
            .checked_mul(16)
            .and_then(|size| size.checked_add(data))
            .ok_or_else(|| format!("GS_COUNT({node_count_in_file}) does not fit in memory"))?;
    }
    Ok(HorizontalShiftGridSet {
        name: name.to_string(),
        grids,
        top_level_indices,
    })
}
