// adapted from PROJ src/projections/labrd.cpp (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::FRAC_PI_4;

use super::{ClassicProjection, EllipsoidConstants, LambdaPhi};
use crate::{Coord, Error};

const EPS: f64 = 1e-10;

pub(super) struct Laborde {
    e: f64,
    one_es: f64,
    phi0: f64,
    k0: f64,
    k_rg: f64,
    p0s: f64,
    a: f64,
    c: f64,
    ca: f64,
    cb: f64,
    cc: f64,
    cd: f64,
}

impl Laborde {
    pub(super) fn new(
        shape: &EllipsoidConstants,
        phi0: f64,
        azimuth: f64,
        k0: f64,
    ) -> Result<Self, Error> {
        if phi0 == 0.0 {
            return Err(Error::ProjectionError(
                "invalid value for lat_0: lat_0 should be different from 0".into(),
            ));
        }
        let az = azimuth;
        let sinp = phi0.sin();
        let mut t = 1.0 - shape.es * sinp * sinp;
        let n = 1.0 / t.sqrt();
        let r = shape.one_es * n / t;
        let k_rg = k0 * (n * r).sqrt();
        let p0s = ((r / n).sqrt() * phi0.tan()).atan();
        let a = sinp / p0s.sin();
        t = shape.e * sinp;
        let c = 0.5 * shape.e * a * ((1.0 + t) / (1.0 - t)).ln()
            + -a * (FRAC_PI_4 + 0.5 * phi0).tan().ln()
            + (FRAC_PI_4 + 0.5 * p0s).tan().ln();
        t = az + az;
        let mut cb = 1.0 / (12.0 * k_rg * k_rg);
        let ca = (1.0 - t.cos()) * cb;
        cb *= t.sin();
        let cc = 3.0 * (ca * ca - cb * cb);
        let cd = 6.0 * ca * cb;
        Ok(Self {
            e: shape.e,
            one_es: shape.one_es,
            phi0,
            k0,
            k_rg,
            p0s,
            a,
            c,
            ca,
            cb,
            cc,
            cd,
        })
    }
}

impl ClassicProjection for Laborde {
    fn forward(&self, lp: LambdaPhi) -> Result<Coord, Error> {
        let a = self.a;
        let mut v1 = a * (FRAC_PI_4 + 0.5 * lp.phi).tan().ln();
        let mut t = self.e * lp.phi.sin();
        let mut v2 = 0.5 * self.e * a * ((1.0 + t) / (1.0 - t)).ln();
        let ps = 2.0 * ((v1 - v2 + self.c).exp().atan() - FRAC_PI_4);
        let i1 = ps - self.p0s;
        let cosps = ps.cos();
        let cosps2 = cosps * cosps;
        let sinps = ps.sin();
        let sinps2 = sinps * sinps;
        let i4 = a * cosps;
        let i2 = 0.5 * a * i4 * sinps;
        let i3 = i2 * a * a * (5.0 * cosps2 - sinps2) / 12.0;
        let mut i6 = i4 * a * a;
        let i5 = i6 * (cosps2 - sinps2) / 6.0;
        i6 *= a * a * (5.0 * cosps2 * cosps2 + sinps2 * (sinps2 - 18.0 * cosps2)) / 120.0;
        t = lp.lambda * lp.lambda;
        let mut x = self.k_rg * lp.lambda * (i4 + t * (i5 + t * i6));
        let mut y = self.k_rg * (i1 + t * (i2 + t * i3));
        let x2 = x * x;
        let y2 = y * y;
        v1 = 3.0 * x * y2 - x * x2;
        v2 = y * y2 - 3.0 * x2 * y;
        x += self.ca * v1 + self.cb * v2;
        y += self.ca * v2 - self.cb * v1;
        Ok(Coord::new(x, y))
    }

    fn inverse(&self, xy: Coord) -> Result<LambdaPhi, Error> {
        let a = self.a;
        let mut x = xy.x;
        let mut y = xy.y;
        let mut x2 = x * x;
        let y2 = y * y;
        let mut v1 = 3.0 * x * y2 - x * x2;
        let mut v2 = y * y2 - 3.0 * x2 * y;
        let v3 = x * (5.0 * y2 * y2 + x2 * (-10.0 * y2 + x2));
        let v4 = y * (5.0 * x2 * x2 + y2 * (-10.0 * x2 + y2));
        x += -self.ca * v1 - self.cb * v2 + self.cc * v3 + self.cd * v4;
        y += self.cb * v1 - self.ca * v2 - self.cd * v3 + self.cc * v4;
        let ps = self.p0s + y / self.k_rg;
        let mut pe = ps + self.phi0 - self.p0s;

        // labrd.cpp stops after 20 iterations without reporting an error
        for _ in 0..20 {
            v1 = a * (FRAC_PI_4 + 0.5 * pe).tan().ln();
            let tpe = self.e * pe.sin();
            v2 = 0.5 * self.e * a * ((1.0 + tpe) / (1.0 - tpe)).ln();
            let t = ps - 2.0 * ((v1 - v2 + self.c).exp().atan() - FRAC_PI_4);
            pe += t;
            if t.abs() < EPS {
                break;
            }
        }

        let mut t = self.e * pe.sin();
        t = 1.0 - t * t;
        let re = self.one_es / (t * t.sqrt());
        t = ps.tan();
        let t2 = t * t;
        let s = self.k_rg * self.k_rg;
        let mut d = re * self.k0 * self.k_rg;
        let i7 = t / (2.0 * d);
        let i8 = t * (5.0 + 3.0 * t2) / (24.0 * d * s);
        d = ps.cos() * self.k_rg * a;
        let i9 = 1.0 / d;
        d *= s;
        let i10 = (1.0 + 2.0 * t2) / (6.0 * d);
        let i11 = (5.0 + t2 * (28.0 + 24.0 * t2)) / (120.0 * d * s);
        x2 = x * x;
        Ok(LambdaPhi {
            lambda: x * (i9 + x2 * (-i10 + x2 * i11)),
            phi: pe + x2 * (-i7 + i8 * x2),
        })
    }
}
