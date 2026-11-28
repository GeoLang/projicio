// adapted from PROJ src/grids.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::PI;

use crate::Error;
use crate::projection::epsg_method::LambdaPhi;
use crate::projection::epsg_method::math::adjlon;

const GRID_SHIFT_RELATIVE_TOLERANCE: f64 = 1e-5;

#[derive(Debug, Clone, Copy)]
pub(super) struct Extent {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
    pub resolution_x: f64,
    pub resolution_y: f64,
}

impl Extent {
    fn epsilon(&self) -> f64 {
        (self.resolution_x + self.resolution_y) * GRID_SHIFT_RELATIVE_TOLERANCE
    }

    fn contains(&self, point: LambdaPhi, epsilon: f64) -> bool {
        // grids.cpp isPointInExtent
        if !(point.phi + epsilon >= self.south && point.phi - epsilon <= self.north) {
            return false;
        }
        if self.east - self.west + self.resolution_x >= 2.0 * PI - 1e-10 {
            return true;
        }
        let mut lambda = point.lambda;
        if lambda + epsilon < self.west {
            lambda += 2.0 * PI;
        } else if lambda - epsilon > self.east {
            lambda -= 2.0 * PI;
        }
        lambda + epsilon >= self.west && lambda - epsilon <= self.east
    }

    fn offset_from_corner(&self, point: LambdaPhi, epsilon: f64) -> LambdaPhi {
        // pj_hgrid_apply_internal's normalisation of its input to the lower left corner
        let mut lambda = point.lambda - self.west;
        if lambda + epsilon < 0.0 {
            lambda += 2.0 * PI;
        } else if lambda - epsilon > self.east - self.west {
            lambda -= 2.0 * PI;
        }
        LambdaPhi {
            lambda,
            phi: point.phi - self.south,
        }
    }
}

fn cell_and_fraction(position: f64, node_count: usize) -> Option<(usize, f64)> {
    // pj_hgrid_interpolate lets a point up to ten tolerances past the first or last node use the edge cell
    let edge_tolerance = 10.0 * GRID_SHIFT_RELATIVE_TOLERANCE;
    let index = if position.is_nan() {
        0.0
    } else {
        position.floor()
    };
    let fraction = position - index;
    let node_count = node_count as f64;
    if index < 0.0 {
        if index != -1.0 || fraction <= 1.0 - edge_tolerance {
            return None;
        }
        return Some((0, 0.0));
    }
    if index + 1.0 >= node_count {
        if index + 1.0 != node_count || fraction >= edge_tolerance {
            return None;
        }
        return Some((index as usize - 1, 1.0));
    }
    Some((index as usize, fraction))
}

pub(super) struct HorizontalShiftGrid {
    pub width: usize,
    pub height: usize,
    pub extent: Extent,
    // radians positive east and north, longitude first, row 0 at the south edge and column 0 at the west edge
    pub shifts: Vec<[f32; 2]>,
    pub child_indices: Vec<usize>,
}

impl HorizontalShiftGrid {
    fn interpolate(&self, offset: LambdaPhi) -> Option<LambdaPhi> {
        // grids.cpp pj_hgrid_interpolate, with the shifts already in the east positive convention
        let (column, fraction_lambda) =
            cell_and_fraction(offset.lambda / self.extent.resolution_x, self.width)?;
        let (row, fraction_phi) =
            cell_and_fraction(offset.phi / self.extent.resolution_y, self.height)?;
        let node = |column: usize, row: usize| self.shifts[row * self.width + column];
        let south_west = node(column, row);
        let south_east = node(column + 1, row);
        let north_west = node(column, row + 1);
        let north_east = node(column + 1, row + 1);

        let weight_south_west = (1.0 - fraction_lambda) * (1.0 - fraction_phi);
        let weight_south_east = fraction_lambda * (1.0 - fraction_phi);
        let weight_north_west = (1.0 - fraction_lambda) * fraction_phi;
        let weight_north_east = fraction_lambda * fraction_phi;
        let weighted = |component: usize| {
            weight_south_west * f64::from(south_west[component])
                + weight_south_east * f64::from(south_east[component])
                + weight_north_west * f64::from(north_west[component])
                + weight_north_east * f64::from(north_east[component])
        };
        Some(LambdaPhi {
            lambda: weighted(0),
            phi: weighted(1),
        })
    }
}

const MAX_ITERATIONS: u32 = 10;
const TOLERANCE: f64 = 1e-12;

struct FirstInterpolation<'a> {
    grid: &'a HorizontalShiftGrid,
    offset: LambdaPhi,
    shift: LambdaPhi,
}

pub(super) struct HorizontalShiftGridSet {
    pub name: String,
    pub grids: Vec<HorizontalShiftGrid>,
    pub top_level_indices: Vec<usize>,
}

impl HorizontalShiftGridSet {
    fn first_containing(
        &self,
        indices: &[usize],
        point: LambdaPhi,
    ) -> Option<&HorizontalShiftGrid> {
        indices
            .iter()
            .map(|&index| &self.grids[index])
            .find(|grid| grid.extent.contains(point, grid.extent.epsilon()))
    }

    fn grid_at(&self, point: LambdaPhi) -> Option<&HorizontalShiftGrid> {
        // grids.cpp HorizontalShiftGridSet::gridAt and HorizontalShiftGrid::gridAt
        let mut grid = self.first_containing(&self.top_level_indices, point)?;
        while let Some(child) = self.first_containing(&grid.child_indices, point) {
            grid = child;
        }
        Some(grid)
    }

    fn outside(&self, point: LambdaPhi) -> Error {
        Error::GridError(format!(
            "longitude {} latitude {} is outside grid {}",
            point.lambda.to_degrees(),
            point.phi.to_degrees(),
            self.name
        ))
    }

    fn first_interpolation(&self, point: LambdaPhi) -> Result<FirstInterpolation<'_>, Error> {
        let grid = self.grid_at(point).ok_or_else(|| self.outside(point))?;
        let offset = grid.extent.offset_from_corner(point, grid.extent.epsilon());
        let shift = grid
            .interpolate(offset)
            .ok_or_else(|| self.outside(point))?;
        Ok(FirstInterpolation {
            grid,
            offset,
            shift,
        })
    }

    pub fn apply_forward(&self, point: LambdaPhi) -> Result<LambdaPhi, Error> {
        // grids.cpp pj_hgrid_apply and pj_hgrid_apply_internal for PJ_FWD
        let first = self.first_interpolation(point)?;
        let shifted = LambdaPhi {
            lambda: point.lambda + first.shift.lambda,
            phi: point.phi + first.shift.phi,
        };
        // pj_hgrid_apply reports an infinite result as outside the grid
        if shifted.lambda.is_infinite() || shifted.phi.is_infinite() {
            return Err(self.outside(point));
        }
        Ok(shifted)
    }

    pub fn apply_inverse(&self, point: LambdaPhi) -> Result<LambdaPhi, Error> {
        // grids.cpp pj_hgrid_apply and pj_hgrid_apply_internal for PJ_INV
        let FirstInterpolation {
            mut grid,
            offset: mut target,
            shift,
        } = self.first_interpolation(point)?;
        // pj_hgrid_apply_internal keeps the first grid's epsilon after it switches grids
        let epsilon = grid.extent.epsilon();
        let mut estimate = LambdaPhi {
            lambda: target.lambda - shift.lambda,
            phi: target.phi - shift.phi,
        };
        let cell_position_fits_int32 =
            |position: f64| position.is_nan() || position.abs() <= f64::from(i32::MAX);
        let mut iterations_left = MAX_ITERATIONS;
        loop {
            // pj_hgrid_interpolate casts cell indices to int32, and cct reports estimates past that range as outside the grid
            if !(cell_position_fits_int32(estimate.lambda / grid.extent.resolution_x)
                && cell_position_fits_int32(estimate.phi / grid.extent.resolution_y))
            {
                return Err(self.outside(point));
            }
            let difference = match grid.interpolate(estimate) {
                Some(shift) => {
                    let difference = LambdaPhi {
                        lambda: estimate.lambda + shift.lambda - target.lambda,
                        phi: estimate.phi + shift.phi - target.phi,
                    };
                    estimate.lambda -= difference.lambda;
                    estimate.phi -= difference.phi;
                    difference
                }
                None => {
                    let estimate_point = LambdaPhi {
                        lambda: estimate.lambda + grid.extent.west,
                        phi: estimate.phi + grid.extent.south,
                    };
                    let Some(next_grid) = self.grid_at(estimate_point) else {
                        break;
                    };
                    if std::ptr::eq(next_grid, grid) {
                        break;
                    }
                    grid = next_grid;
                    estimate = LambdaPhi {
                        lambda: estimate_point.lambda - grid.extent.west,
                        phi: estimate_point.phi - grid.extent.south,
                    };
                    target = grid.extent.offset_from_corner(point, epsilon);
                    LambdaPhi {
                        lambda: f64::MAX,
                        phi: f64::MAX,
                    }
                }
            };
            iterations_left -= 1;
            let squared_difference =
                difference.lambda * difference.lambda + difference.phi * difference.phi;
            if iterations_left == 0 || squared_difference <= TOLERANCE * TOLERANCE {
                break;
            }
        }
        // pj_hgrid_apply_internal decrements before its convergence test, so a run converging on the tenth pass also fails
        if iterations_left == 0 {
            return Err(Error::GridError(format!(
                "inverse shift through grid {} did not converge at longitude {} latitude {}",
                self.name,
                point.lambda.to_degrees(),
                point.phi.to_degrees()
            )));
        }
        Ok(LambdaPhi {
            lambda: adjlon(estimate.lambda + grid.extent.west),
            phi: estimate.phi + grid.extent.south,
        })
    }
}
