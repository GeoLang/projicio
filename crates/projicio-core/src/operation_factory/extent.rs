// adapted from PROJ src/iso19111/metadata.cpp, src/iso19111/operation/coordinateoperationfactory.cpp and src/iso19111/factory.cpp (MIT), see LICENSES/PROJ-MIT.txt

use crate::Error;
use crate::iso19111::Crs;
use crate::iso19111::common::ObjectDomain;
use crate::iso19111::metadata::{Extent, GeographicBoundingBox};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl BoundingBox {
    pub(crate) fn new(west: f64, south: f64, east: f64, north: f64) -> Result<Self, Error> {
        // proj's GeographicBoundingBox::create widens a box reduced to a point or a line
        let widened = GeographicBoundingBox::new(west, south, east, north)?;
        Ok(Self {
            west: widened.west_longitude,
            south: widened.south_latitude,
            east: widened.east_longitude,
            north: widened.north_latitude,
        })
    }

    pub(crate) fn is_world(&self) -> bool {
        self.west == -180.0 && self.east == 180.0 && self.south == -90.0 && self.north == 90.0
    }

    pub fn contains(&self, other: &BoundingBox) -> bool {
        let (w, e, n, s) = (self.west, self.east, self.north, self.south);
        let (ow, oe, on, os) = (other.west, other.east, other.north, other.south);
        if !(s <= os && n >= on) {
            return false;
        }
        if w == -180.0 && e == 180.0 {
            return ow != oe;
        }
        if ow == -180.0 && oe == 180.0 {
            return false;
        }
        if w < e {
            return ow < oe && w <= ow && e >= oe;
        }
        if ow < oe {
            return ow >= w || oe <= e;
        }
        w <= ow && e >= oe
    }

    pub fn intersects(&self, other: &BoundingBox) -> bool {
        let (w, e, n, s) = (self.west, self.east, self.north, self.south);
        let (ow, oe, on, os) = (other.west, other.east, other.north, other.south);
        if n < os || s > on {
            return false;
        }
        if w == -180.0 && e == 180.0 && ow > oe {
            return true;
        }
        if ow == -180.0 && oe == 180.0 && w > e {
            return true;
        }
        if w <= e {
            if ow <= oe {
                return w.max(ow) < e.min(oe);
            }
            if ow > 180.0 || oe < -180.0 {
                return false;
            }
            return self.intersects(&BoundingBox::raw(ow, os, 180.0, on))
                || self.intersects(&BoundingBox::raw(-180.0, os, oe, on));
        }
        if ow <= oe {
            return other.intersects(self);
        }
        true
    }

    fn raw(west: f64, south: f64, east: f64, north: f64) -> Self {
        Self {
            west,
            south,
            east,
            north,
        }
    }

    fn intersection(&self, other: &BoundingBox) -> Option<BoundingBox> {
        let (w, e, n, s) = (self.west, self.east, self.north, self.south);
        let (ow, oe, on, os) = (other.west, other.east, other.north, other.south);
        if n < os || s > on {
            return None;
        }
        if w == -180.0 && e == 180.0 && ow > oe {
            return Some(BoundingBox::raw(ow, s.max(os), oe, n.min(on)));
        }
        if ow == -180.0 && oe == 180.0 && w > e {
            return Some(BoundingBox::raw(w, s.max(os), e, n.min(on)));
        }
        if w <= e {
            if ow <= oe {
                let west = w.max(ow);
                let east = e.min(oe);
                return (west < east).then(|| BoundingBox::raw(west, s.max(os), east, n.min(on)));
            }
            if ow > 180.0 || oe < -180.0 {
                return None;
            }
            // proj keeps the larger of the two parts the antimeridian splits the other box into
            let east_part = self.intersection(&BoundingBox::raw(ow, os, 180.0, on));
            let west_part = self.intersection(&BoundingBox::raw(-180.0, os, oe, on));
            return match (east_part, west_part) {
                (None, part) | (part, None) => part,
                (Some(east_part), Some(west_part)) => {
                    if east_part.east - east_part.west > west_part.east - west_part.west {
                        Some(east_part)
                    } else {
                        Some(west_part)
                    }
                }
            };
        }
        if ow <= oe {
            return other.intersection(self);
        }
        Some(BoundingBox::raw(w.max(ow), s.max(os), e.min(oe), n.min(on)))
    }

    pub(crate) fn pseudo_area(&self) -> f64 {
        let mut east = self.east;
        if self.west > east {
            east += 360.0;
        }
        // integrates cos(lat) between the south and north latitudes
        (east - self.west) * (self.north.to_radians().sin() - self.south.to_radians().sin())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AreaOfUse {
    pub description: Option<String>,
    pub bounding_box: BoundingBox,
}

impl AreaOfUse {
    pub fn from_bounding_box(west: f64, south: f64, east: f64, north: f64) -> Result<Self, Error> {
        Ok(Self {
            description: None,
            bounding_box: BoundingBox::new(west, south, east, north)?,
        })
    }

    pub(crate) fn world() -> Self {
        Self {
            description: Some("World".to_string()),
            bounding_box: BoundingBox::raw(-180.0, -90.0, 180.0, 90.0),
        }
    }

    pub(crate) fn from_extent(extent: &Extent) -> Option<Self> {
        let bounding_box = extent.geographic_bounding_box.as_ref()?;
        Some(Self {
            description: extent.description.clone(),
            bounding_box: BoundingBox::raw(
                bounding_box.west_longitude,
                bounding_box.south_latitude,
                bounding_box.east_longitude,
                bounding_box.north_latitude,
            ),
        })
    }

    pub(crate) fn from_domains(domains: &[ObjectDomain]) -> Option<Self> {
        domains
            .first()?
            .domain_of_validity
            .as_ref()
            .and_then(AreaOfUse::from_extent)
    }

    pub(crate) fn to_domain(&self) -> ObjectDomain {
        let bounding_box = &self.bounding_box;
        ObjectDomain {
            scope: None,
            domain_of_validity: Some(Extent {
                description: self.description.clone(),
                geographic_bounding_box: Some(GeographicBoundingBox {
                    west_longitude: bounding_box.west,
                    south_latitude: bounding_box.south,
                    east_longitude: bounding_box.east,
                    north_latitude: bounding_box.north,
                }),
                vertical_extent: None,
                temporal_extent: None,
            }),
        }
    }

    pub fn contains(&self, other: &AreaOfUse) -> bool {
        self.bounding_box.contains(&other.bounding_box)
    }

    pub fn intersects(&self, other: &AreaOfUse) -> bool {
        self.bounding_box.intersects(&other.bounding_box)
    }

    pub(crate) fn intersection(&self, other: &AreaOfUse) -> Option<AreaOfUse> {
        // proj's Extent::intersection hands back a whole extent, description included, when one holds the other
        if self.contains(other) {
            return Some(other.clone());
        }
        if other.contains(self) {
            return Some(self.clone());
        }
        let bounding_box = self.bounding_box.intersection(&other.bounding_box)?;
        Some(AreaOfUse {
            description: None,
            bounding_box: BoundingBox::new(
                bounding_box.west,
                bounding_box.south,
                bounding_box.east,
                bounding_box.north,
            )
            .expect("an intersection keeps its south bound below its north bound"),
        })
    }
}

pub(crate) fn crs_area_of_use(crs: &Crs) -> Option<AreaOfUse> {
    // proj's getExtent(crs): the first usage, and a bound CRS's base
    let domains = match crs {
        Crs::Geographic(geographic) => &geographic.domains,
        Crs::Geodetic(geodetic) => &geodetic.domains,
        Crs::Projected(projected) => &projected.domains,
        Crs::Bound(bound) => {
            return AreaOfUse::from_domains(&bound.domains)
                .or_else(|| crs_area_of_use(&bound.source_crs));
        }
    };
    AreaOfUse::from_domains(domains)
}

pub(crate) fn pseudo_area(area: Option<&AreaOfUse>) -> f64 {
    area.map_or(0.0, |area| area.bounding_box.pseudo_area())
}

pub(crate) fn registry_area(extent_code: u32) -> Result<AreaOfUse, Error> {
    let extent =
        crate::registry::extent(extent_code).ok_or_else(|| Error::NoSuchAuthorityCode {
            kind: "extent",
            code: format!("EPSG:{extent_code}"),
        })?;
    Ok(AreaOfUse {
        description: Some(extent.name.to_string()),
        bounding_box: BoundingBox::new(extent.west, extent.south, extent.east, extent.north)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_crossing_the_antimeridian_contains_a_box_on_either_side() {
        let pacific = BoundingBox::raw(170.0, -10.0, -170.0, 10.0);
        assert!(pacific.contains(&BoundingBox::raw(175.0, -5.0, 178.0, 5.0)));
        assert!(pacific.contains(&BoundingBox::raw(-178.0, -5.0, -175.0, 5.0)));
        assert!(!pacific.contains(&BoundingBox::raw(0.0, -5.0, 10.0, 5.0)));
    }

    #[test]
    fn intersection_keeps_the_contained_extent_with_its_description() {
        let world = AreaOfUse::world();
        let france = AreaOfUse {
            description: Some("France".to_string()),
            bounding_box: BoundingBox::raw(-5.0, 41.0, 10.0, 51.0),
        };
        assert_eq!(world.intersection(&france), Some(france.clone()));
        let overlap = france
            .intersection(&AreaOfUse::from_bounding_box(5.0, 45.0, 20.0, 60.0).unwrap())
            .unwrap();
        assert_eq!(overlap.description, None);
        assert_eq!(
            overlap.bounding_box,
            BoundingBox::raw(5.0, 45.0, 10.0, 51.0)
        );
    }

    #[test]
    fn pseudo_area_wraps_a_box_crossing_the_antimeridian() {
        let wrapped = BoundingBox::raw(170.0, 0.0, -170.0, 90.0);
        assert!((wrapped.pseudo_area() - 20.0).abs() < 1e-12);
    }
}
