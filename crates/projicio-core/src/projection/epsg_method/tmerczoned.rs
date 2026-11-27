// adapted from PROJ src/projections/tmerczoned.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::PI;

use super::tmerc::TransverseMercator;
use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

pub(super) struct TransverseMercatorZoned {
    tmerc: TransverseMercator,
    zone_width: f64,
    per_zone_easting_offset: f64,
}

impl TransverseMercatorZoned {
    pub(super) fn new(shape: &EllipsoidConstants, k0: f64, zone_width: f64) -> Result<Self, Error> {
        if zone_width.is_nan() || zone_width <= 0.0 {
            return Err(Error::ProjectionError(format!(
                "invalid zone width {zone_width}"
            )));
        }
        // tmerczoned.cpp builds its inner tmerc with a zero latitude of origin
        Ok(Self {
            tmerc: TransverseMercator::new(shape, 0.0, k0),
            zone_width,
            per_zone_easting_offset: 1_000_000.0 / shape.a,
        })
    }

    fn center_lambda(&self, zone: i32) -> f64 {
        (f64::from(zone) - 0.5) * self.zone_width
    }
}

impl ClassicProjection for TransverseMercatorZoned {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let mut lambda = lp.lambda;
        if lambda < 0.0 {
            lambda += 2.0 * PI;
        }
        // tmerczoned.cpp nudges the zone's lower bound to be inclusive
        const EPS: f64 = 1e-12;
        let zone = (lambda / self.zone_width + EPS) as i32 + 1;
        lambda -= self.center_lambda(zone);
        let xy = self.tmerc.forward(LambdaPhi {
            lambda,
            phi: lp.phi,
        })?;
        Ok(Coord::new(
            xy.x + f64::from(zone) * self.per_zone_easting_offset,
            xy.y,
        ))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let zone = (xy.x / self.per_zone_easting_offset + 0.5) as i32;
        let lp = self.tmerc.inverse(Coord::new(
            xy.x - f64::from(zone) * self.per_zone_easting_offset,
            xy.y,
        ))?;
        Ok(LambdaPhi {
            lambda: lp.lambda + self.center_lambda(zone),
            phi: lp.phi,
        })
    }
}
