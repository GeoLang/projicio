// adapted from PROJ src/geodesic.c (MIT), see LICENSES/PROJ-MIT.txt

use std::f64::consts::PI;

const ORDER: usize = 6;
const N_C: usize = ORDER + 1;
const N_C3X: usize = (ORDER * (ORDER - 1)) / 2;

const QD: f64 = 90.0;
const HD: f64 = 180.0;
const TD: f64 = 360.0;

const DEGREE: f64 = PI / HD;
// sqrt(DBL_MIN), exactly 2^-511
const TINY: f64 = 1.4916681462400413e-154;
const TOL0: f64 = f64::EPSILON;
const TOL1: f64 = 200.0 * TOL0;
// sqrt(DBL_EPSILON), exactly 2^-26
const TOL2: f64 = 1.4901161193847656e-8;
const TOLB: f64 = TOL0;
const XTHRESH: f64 = 1000.0 * TOL2;
const MAXIT1: u32 = 20;
// geodesic.c adds DBL_MANT_DIG, 53 for IEEE doubles
const MAXIT2: u32 = MAXIT1 + 53 + 10;

pub(super) struct Geodesic {
    a: f64,
    f: f64,
    f1: f64,
    ep2: f64,
    n: f64,
    b: f64,
    etol2: f64,
    a3x: [f64; ORDER],
    c3x: [f64; N_C3X],
}

pub(super) struct GeodesicInverse {
    pub(super) s12: f64,
    pub(super) azi1: f64,
}

pub(super) struct GeodesicDirect {
    pub(super) lat2: f64,
    pub(super) lon2: f64,
}

fn sq(x: f64) -> f64 {
    x * x
}

fn sumx(u: f64, v: f64) -> (f64, f64) {
    let s = u + v;
    let mut up = s - v;
    let mut vpp = s - up;
    up -= u;
    vpp -= v;
    let t = if s != 0.0 { 0.0 - (up + vpp) } else { s };
    (s, t)
}

fn polyval_descending(p: &[f64], x: f64) -> f64 {
    let mut y = p[0];
    for &coefficient in &p[1..] {
        y = y * x + coefficient;
    }
    y
}

fn norm2(sinx: &mut f64, cosx: &mut f64) {
    let r = sinx.hypot(*cosx);
    *sinx /= r;
    *cosx /= r;
}

fn remainder_with_quotient(x: f64, y: f64) -> (f64, f64) {
    // C remainder and remquo round the quotient to nearest with ties to even, Rust's % truncates it
    let mut r = x % y;
    let half = y / 2.0;
    let truncated_quotient = (x - r) / y;
    let mut quotient = truncated_quotient;
    if r.abs() > half {
        quotient += 1.0_f64.copysign(r);
        r -= y.copysign(r);
    } else if r.abs() == half && truncated_quotient % 2.0 != 0.0 {
        quotient += 1.0_f64.copysign(r);
        r = -r;
    }
    (r, quotient)
}

fn remainder(x: f64, y: f64) -> f64 {
    remainder_with_quotient(x, y).0
}

fn ang_normalize(x: f64) -> f64 {
    let y = remainder(x, TD);
    if y.abs() == HD { HD.copysign(x) } else { y }
}

fn lat_fix(x: f64) -> f64 {
    if x.abs() > QD { f64::NAN } else { x }
}

fn ang_diff(x: f64, y: f64) -> (f64, f64) {
    let (d, t) = sumx(remainder(-x, TD), remainder(y, TD));
    let (mut d, t) = sumx(remainder(d, TD), t);
    if d == 0.0 || d.abs() == HD {
        d = d.copysign(if t == 0.0 { y - x } else { -t });
    }
    (d, t)
}

fn ang_round(x: f64) -> f64 {
    let z = 1.0 / 16.0;
    let mut y = x.abs();
    let w = z - y;
    y = if w > 0.0 { z - w } else { y };
    y.copysign(x)
}

fn sincos_quadrant(r: f64, quotient: f64, x: f64) -> (f64, f64) {
    let r = r * DEGREE;
    let s = r.sin();
    let c = r.cos();
    let (sinx, cosx) = match quotient.rem_euclid(4.0) as u32 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    let cosx = cosx + 0.0;
    let sinx = if sinx == 0.0 { sinx.copysign(x) } else { sinx };
    (sinx, cosx)
}

fn sincosdx(x: f64) -> (f64, f64) {
    let (r, quotient) = remainder_with_quotient(x, QD);
    sincos_quadrant(r, quotient, x)
}

fn sincosde(x: f64, t: f64) -> (f64, f64) {
    let (r, quotient) = remainder_with_quotient(x, QD);
    sincos_quadrant(ang_round(r + t), quotient, x)
}

fn atan2dx(y: f64, x: f64) -> f64 {
    let (mut y, mut x) = (y, x);
    let mut q = 0;
    if y.abs() > x.abs() {
        std::mem::swap(&mut x, &mut y);
        q = 2;
    }
    if x.is_sign_negative() {
        x = -x;
        q += 1;
    }
    let ang = y.atan2(x) / DEGREE;
    match q {
        1 => HD.copysign(y) - ang,
        2 => QD - ang,
        3 => -QD + ang,
        _ => ang,
    }
}

fn sin_series(sinx: f64, cosx: f64, c: &[f64], n: usize) -> f64 {
    let mut k = n + 1;
    let ar = 2.0 * (cosx - sinx) * (cosx + sinx);
    let mut y0 = if n & 1 == 1 {
        k -= 1;
        c[k]
    } else {
        0.0
    };
    let mut y1 = 0.0;
    for _ in 0..n / 2 {
        k -= 1;
        y1 = ar * y0 - y1 + c[k];
        k -= 1;
        y0 = ar * y1 - y0 + c[k];
    }
    2.0 * sinx * cosx * y0
}

fn a1m1f(eps: f64) -> f64 {
    const COEFF: [f64; 5] = [1.0, 4.0, 64.0, 0.0, 256.0];
    let m = ORDER / 2;
    let t = polyval_descending(&COEFF[..=m], sq(eps)) / COEFF[m + 1];
    (t + eps) / (1.0 - eps)
}

fn fourier_coefficients(coeff: &[f64], eps: f64) -> [f64; N_C] {
    let mut c = [0.0; N_C];
    let eps2 = sq(eps);
    let mut d = eps;
    let mut o = 0;
    for (l, value) in c.iter_mut().enumerate().skip(1) {
        let m = (ORDER - l) / 2;
        *value = d * polyval_descending(&coeff[o..=o + m], eps2) / coeff[o + m + 1];
        o += m + 2;
        d *= eps;
    }
    c
}

#[rustfmt::skip]
fn c1f(eps: f64) -> [f64; N_C] {
    const COEFF: [f64; 18] = [
        -1.0, 6.0, -16.0, 32.0,
        -9.0, 64.0, -128.0, 2048.0,
        9.0, -16.0, 768.0,
        3.0, -5.0, 512.0,
        -7.0, 1280.0,
        -7.0, 2048.0,
    ];
    fourier_coefficients(&COEFF, eps)
}

#[rustfmt::skip]
fn c1pf(eps: f64) -> [f64; N_C] {
    const COEFF: [f64; 18] = [
        205.0, -432.0, 768.0, 1536.0,
        4005.0, -4736.0, 3840.0, 12288.0,
        -225.0, 116.0, 384.0,
        -7173.0, 2695.0, 7680.0,
        3467.0, 7680.0,
        38081.0, 61440.0,
    ];
    fourier_coefficients(&COEFF, eps)
}

fn a2m1f(eps: f64) -> f64 {
    const COEFF: [f64; 5] = [-11.0, -28.0, -192.0, 0.0, 256.0];
    let m = ORDER / 2;
    let t = polyval_descending(&COEFF[..=m], sq(eps)) / COEFF[m + 1];
    (t - eps) / (1.0 + eps)
}

#[rustfmt::skip]
fn c2f(eps: f64) -> [f64; N_C] {
    const COEFF: [f64; 18] = [
        1.0, 2.0, 16.0, 32.0,
        35.0, 64.0, 384.0, 2048.0,
        15.0, 80.0, 768.0,
        7.0, 35.0, 512.0,
        63.0, 1280.0,
        77.0, 2048.0,
    ];
    fourier_coefficients(&COEFF, eps)
}

#[rustfmt::skip]
fn a3coeff(n: f64) -> [f64; ORDER] {
    const COEFF: [f64; 18] = [
        -3.0, 128.0,
        -2.0, -3.0, 64.0,
        -1.0, -3.0, -1.0, 16.0,
        3.0, -1.0, -2.0, 8.0,
        1.0, -1.0, 2.0,
        1.0, 1.0,
    ];
    let mut a3x = [0.0; ORDER];
    let mut o = 0;
    for (k, j) in (0..ORDER).rev().enumerate() {
        let m = (ORDER - j - 1).min(j);
        a3x[k] = polyval_descending(&COEFF[o..=o + m], n) / COEFF[o + m + 1];
        o += m + 2;
    }
    a3x
}

#[rustfmt::skip]
fn c3coeff(n: f64) -> [f64; N_C3X] {
    const COEFF: [f64; 45] = [
        3.0, 128.0,
        2.0, 5.0, 128.0,
        -1.0, 3.0, 3.0, 64.0,
        -1.0, 0.0, 1.0, 8.0,
        -1.0, 1.0, 4.0,
        5.0, 256.0,
        1.0, 3.0, 128.0,
        -3.0, -2.0, 3.0, 64.0,
        1.0, -3.0, 2.0, 32.0,
        7.0, 512.0,
        -10.0, 9.0, 384.0,
        5.0, -9.0, 5.0, 192.0,
        7.0, 512.0,
        -14.0, 7.0, 512.0,
        21.0, 2560.0,
    ];
    let mut c3x = [0.0; N_C3X];
    let mut o = 0;
    let mut k = 0;
    for l in 1..ORDER {
        for j in (l..ORDER).rev() {
            let m = (ORDER - j - 1).min(j);
            c3x[k] = polyval_descending(&COEFF[o..=o + m], n) / COEFF[o + m + 1];
            k += 1;
            o += m + 2;
        }
    }
    c3x
}

fn astroid(x: f64, y: f64) -> f64 {
    let p = sq(x);
    let q = sq(y);
    let r = (p + q - 1.0) / 6.0;
    if q == 0.0 && r <= 0.0 {
        return 0.0;
    }
    let s = p * q / 4.0;
    let r2 = sq(r);
    let r3 = r * r2;
    let disc = s * (s + 2.0 * r3);
    let mut u = r;
    if disc >= 0.0 {
        let mut t3 = s + r3;
        t3 += if t3 < 0.0 { -disc.sqrt() } else { disc.sqrt() };
        let t = t3.cbrt();
        u += t + if t != 0.0 { r2 / t } else { 0.0 };
    } else {
        let ang = (-disc).sqrt().atan2(-(s + r3));
        u += 2.0 * r * (ang / 3.0).cos();
    }
    let v = (sq(u) + q).sqrt();
    let uv = if u < 0.0 { q / (v - u) } else { u + v };
    let w = (uv - q) / (2.0 * v);
    uv / ((uv + sq(w)).sqrt() + w)
}

struct DistanceAndReducedLength {
    s12b: f64,
    m12b: f64,
}

struct ReducedLength {
    m12b: f64,
    m0: f64,
}

struct SigmaEndpoints {
    ssig1: f64,
    csig1: f64,
    dn1: f64,
    ssig2: f64,
    csig2: f64,
    dn2: f64,
}

struct InverseSolution {
    salp1: f64,
    calp1: f64,
    salp2: f64,
    calp2: f64,
    s12x: f64,
}

struct InverseStart {
    sig12: f64,
    salp1: f64,
    calp1: f64,
    salp2: f64,
    calp2: f64,
    dnm: f64,
}

struct Lambda12 {
    lam12: f64,
    salp2: f64,
    calp2: f64,
    sig12: f64,
    ssig1: f64,
    csig1: f64,
    ssig2: f64,
    csig2: f64,
    eps: f64,
}

#[derive(Clone, Copy)]
struct ReducedLatitudes {
    sbet1: f64,
    cbet1: f64,
    dn1: f64,
    sbet2: f64,
    cbet2: f64,
    dn2: f64,
}

fn distance_and_reduced_length(
    eps: f64,
    sig12: f64,
    sigma: &SigmaEndpoints,
) -> DistanceAndReducedLength {
    // geodesic.c Lengths with both ps12b and pm12b requested
    let mut a1 = a1m1f(eps);
    let ca = c1f(eps);
    let mut a2 = a2m1f(eps);
    let cb = c2f(eps);
    let m0 = a1 - a2;
    a2 += 1.0;
    a1 += 1.0;
    let b1 = sin_series(sigma.ssig2, sigma.csig2, &ca, ORDER)
        - sin_series(sigma.ssig1, sigma.csig1, &ca, ORDER);
    let s12b = a1 * (sig12 + b1);
    let b2 = sin_series(sigma.ssig2, sigma.csig2, &cb, ORDER)
        - sin_series(sigma.ssig1, sigma.csig1, &cb, ORDER);
    let j12 = m0 * sig12 + (a1 * b1 - a2 * b2);
    let m12b = sigma.dn2 * (sigma.csig1 * sigma.ssig2)
        - sigma.dn1 * (sigma.ssig1 * sigma.csig2)
        - sigma.csig1 * sigma.csig2 * j12;
    DistanceAndReducedLength { s12b, m12b }
}

fn reduced_length(eps: f64, sig12: f64, sigma: &SigmaEndpoints) -> ReducedLength {
    // geodesic.c Lengths with ps12b null sums the two series in a different order
    let mut a1 = a1m1f(eps);
    let ca = c1f(eps);
    let mut a2 = a2m1f(eps);
    let mut cb = c2f(eps);
    let m0 = a1 - a2;
    a2 += 1.0;
    a1 += 1.0;
    for (cb_l, ca_l) in cb.iter_mut().zip(ca.iter()).skip(1) {
        *cb_l = a1 * ca_l - a2 * *cb_l;
    }
    let j12 = m0 * sig12
        + (sin_series(sigma.ssig2, sigma.csig2, &cb, ORDER)
            - sin_series(sigma.ssig1, sigma.csig1, &cb, ORDER));
    let m12b = sigma.dn2 * (sigma.csig1 * sigma.ssig2)
        - sigma.dn1 * (sigma.ssig1 * sigma.csig2)
        - sigma.csig1 * sigma.csig2 * j12;
    ReducedLength { m12b, m0 }
}

impl Geodesic {
    pub(super) fn new(f: f64) -> Self {
        // aeqd.cpp runs the geodesic on a unit semi-major axis
        let a = 1.0;
        let f1 = 1.0 - f;
        let e2 = f * (2.0 - f);
        let n = f / (2.0 - f);
        Self {
            a,
            f,
            f1,
            ep2: e2 / sq(f1),
            n,
            b: a * f1,
            etol2: 0.1 * TOL2
                / ((0.001_f64.max(f.abs()) * 1.0_f64.min(1.0 - f / 2.0)) / 2.0).sqrt(),
            a3x: a3coeff(n),
            c3x: c3coeff(n),
        }
    }

    fn a3f(&self, eps: f64) -> f64 {
        polyval_descending(&self.a3x, eps)
    }

    fn c3f(&self, eps: f64) -> [f64; N_C] {
        let mut c = [0.0; N_C];
        let mut mult = 1.0;
        let mut o = 0;
        for (l, value) in c.iter_mut().enumerate().take(ORDER).skip(1) {
            let m = ORDER - l - 1;
            mult *= eps;
            *value = mult * polyval_descending(&self.c3x[o..=o + m], eps);
            o += m + 1;
        }
        c
    }

    fn inverse_start(
        &self,
        points: &ReducedLatitudes,
        lam12: f64,
        slam12: f64,
        clam12: f64,
    ) -> InverseStart {
        let ReducedLatitudes {
            sbet1,
            cbet1,
            dn1,
            sbet2,
            cbet2,
            dn2,
        } = *points;
        let mut salp2 = 0.0;
        let mut calp2 = 0.0;
        let mut dnm = 0.0;
        let mut sig12 = -1.0;
        let sbet12 = sbet2 * cbet1 - cbet2 * sbet1;
        let cbet12 = cbet2 * cbet1 + sbet2 * sbet1;
        let shortline = cbet12 >= 0.0 && sbet12 < 0.5 && cbet2 * lam12 < 0.5;
        let sbet12a = sbet2 * cbet1 + cbet2 * sbet1;
        let (mut somg12, mut comg12);
        if shortline {
            let mut sbetm2 = sq(sbet1 + sbet2);
            sbetm2 /= sbetm2 + sq(cbet1 + cbet2);
            dnm = (1.0 + self.ep2 * sbetm2).sqrt();
            let omg12 = lam12 / (self.f1 * dnm);
            somg12 = omg12.sin();
            comg12 = omg12.cos();
        } else {
            somg12 = slam12;
            comg12 = clam12;
        }

        let mut salp1 = cbet2 * somg12;
        let mut calp1 = if comg12 >= 0.0 {
            sbet12 + cbet2 * sbet1 * sq(somg12) / (1.0 + comg12)
        } else {
            sbet12a - cbet2 * sbet1 * sq(somg12) / (1.0 - comg12)
        };

        let ssig12 = salp1.hypot(calp1);
        let csig12 = sbet1 * sbet2 + cbet1 * cbet2 * comg12;

        if shortline && ssig12 < self.etol2 {
            salp2 = cbet1 * somg12;
            calp2 = sbet12
                - cbet1
                    * sbet2
                    * if comg12 >= 0.0 {
                        sq(somg12) / (1.0 + comg12)
                    } else {
                        1.0 - comg12
                    };
            norm2(&mut salp2, &mut calp2);
            sig12 = ssig12.atan2(csig12);
        } else if !(self.n.abs() > 0.1
            || csig12 >= 0.0
            || ssig12 >= 6.0 * self.n.abs() * PI * sq(cbet1))
        {
            let (x, y, lamscale);
            let lam12x = (-slam12).atan2(-clam12);
            if self.f >= 0.0 {
                let k2 = sq(sbet1) * self.ep2;
                let eps = k2 / (2.0 * (1.0 + (1.0 + k2).sqrt()) + k2);
                lamscale = self.f * cbet1 * self.a3f(eps) * PI;
                let betscale = lamscale * cbet1;
                x = lam12x / lamscale;
                y = sbet12a / betscale;
            } else {
                let cbet12a = cbet2 * cbet1 - sbet2 * sbet1;
                let bet12a = sbet12a.atan2(cbet12a);
                let lengths = reduced_length(
                    self.n,
                    PI + bet12a,
                    &SigmaEndpoints {
                        ssig1: sbet1,
                        csig1: -cbet1,
                        dn1,
                        ssig2: sbet2,
                        csig2: cbet2,
                        dn2,
                    },
                );
                x = -1.0 + lengths.m12b / (cbet1 * cbet2 * lengths.m0 * PI);
                let betscale = if x < -0.01 {
                    sbet12a / x
                } else {
                    -self.f * sq(cbet1) * PI
                };
                lamscale = betscale / cbet1;
                y = lam12x / lamscale;
            }

            if y > -TOL1 && x > -1.0 - XTHRESH {
                if self.f >= 0.0 {
                    salp1 = 1.0_f64.min(-x);
                    calp1 = -(1.0 - sq(salp1)).sqrt();
                } else {
                    calp1 = (if x > -TOL1 { 0.0_f64 } else { -1.0 }).max(x);
                    salp1 = (1.0 - sq(calp1)).sqrt();
                }
            } else {
                let k = astroid(x, y);
                let omg12a = lamscale
                    * if self.f >= 0.0 {
                        -x * k / (1.0 + k)
                    } else {
                        -y * (1.0 + k) / k
                    };
                somg12 = omg12a.sin();
                comg12 = -omg12a.cos();
                salp1 = cbet2 * somg12;
                calp1 = sbet12a - cbet2 * sbet1 * sq(somg12) / (1.0 - comg12);
            }
        }
        // geodesic.c tests !(salp1 <= 0) so a NaN is normalized
        if salp1 > 0.0 || salp1.is_nan() {
            norm2(&mut salp1, &mut calp1);
        } else {
            salp1 = 1.0;
            calp1 = 0.0;
        }

        InverseStart {
            sig12,
            salp1,
            calp1,
            salp2,
            calp2,
            dnm,
        }
    }

    fn lambda12(
        &self,
        points: &ReducedLatitudes,
        salp1: f64,
        calp1: f64,
        slam120: f64,
        clam120: f64,
    ) -> Lambda12 {
        let ReducedLatitudes {
            sbet1,
            cbet1,
            sbet2,
            cbet2,
            ..
        } = *points;
        let calp1 = if sbet1 == 0.0 && calp1 == 0.0 {
            -TINY
        } else {
            calp1
        };

        let salp0 = salp1 * cbet1;
        let calp0 = calp1.hypot(salp1 * sbet1);

        let mut ssig1 = sbet1;
        let somg1 = salp0 * sbet1;
        let mut csig1 = calp1 * cbet1;
        let comg1 = csig1;
        norm2(&mut ssig1, &mut csig1);

        let salp2 = if cbet2 != cbet1 { salp0 / cbet2 } else { salp1 };
        let calp2 = if cbet2 != cbet1 || sbet2.abs() != -sbet1 {
            (sq(calp1 * cbet1)
                + if cbet1 < -sbet1 {
                    (cbet2 - cbet1) * (cbet1 + cbet2)
                } else {
                    (sbet1 - sbet2) * (sbet1 + sbet2)
                })
            .sqrt()
                / cbet2
        } else {
            calp1.abs()
        };
        let mut ssig2 = sbet2;
        let somg2 = salp0 * sbet2;
        let mut csig2 = calp2 * cbet2;
        let comg2 = csig2;
        norm2(&mut ssig2, &mut csig2);

        let sig12 =
            (0.0_f64.max(csig1 * ssig2 - ssig1 * csig2) + 0.0).atan2(csig1 * csig2 + ssig1 * ssig2);

        let somg12 = 0.0_f64.max(comg1 * somg2 - somg1 * comg2) + 0.0;
        let comg12 = comg1 * comg2 + somg1 * somg2;
        let eta = (somg12 * clam120 - comg12 * slam120).atan2(comg12 * clam120 + somg12 * slam120);
        let k2 = sq(calp0) * self.ep2;
        let eps = k2 / (2.0 * (1.0 + (1.0 + k2).sqrt()) + k2);
        let ca = self.c3f(eps);
        let b312 =
            sin_series(ssig2, csig2, &ca, ORDER - 1) - sin_series(ssig1, csig1, &ca, ORDER - 1);
        let domg12 = -self.f * self.a3f(eps) * salp0 * (sig12 + b312);
        let lam12 = eta + domg12;

        Lambda12 {
            lam12,
            salp2,
            calp2,
            sig12,
            ssig1,
            csig1,
            ssig2,
            csig2,
            eps,
        }
    }

    fn lambda12_derivative(&self, points: &ReducedLatitudes, lambda: &Lambda12) -> f64 {
        if lambda.calp2 == 0.0 {
            return -2.0 * self.f1 * points.dn1 / points.sbet1;
        }
        let lengths = reduced_length(
            lambda.eps,
            lambda.sig12,
            &SigmaEndpoints {
                ssig1: lambda.ssig1,
                csig1: lambda.csig1,
                dn1: points.dn1,
                ssig2: lambda.ssig2,
                csig2: lambda.csig2,
                dn2: points.dn2,
            },
        );
        lengths.m12b * (self.f1 / (lambda.calp2 * points.cbet2))
    }

    pub(super) fn inverse(&self, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> GeodesicInverse {
        let (mut lon12, mut lon12s) = ang_diff(lon1, lon2);
        let mut lonsign = if lon12.is_sign_negative() { -1.0 } else { 1.0 };
        lon12 *= lonsign;
        lon12s *= lonsign;
        let lam12 = lon12 * DEGREE;
        let (slam12, clam12) = sincosde(lon12, lon12s);
        lon12s = (HD - lon12) - lon12s;

        let mut lat1 = ang_round(lat_fix(lat1));
        let mut lat2 = ang_round(lat_fix(lat2));
        let swapp = if lat1.abs() < lat2.abs() || lat2.is_nan() {
            -1.0
        } else {
            1.0
        };
        if swapp < 0.0 {
            lonsign *= -1.0;
            std::mem::swap(&mut lat1, &mut lat2);
        }
        let latsign = if lat1.is_sign_negative() { 1.0 } else { -1.0 };
        lat1 *= latsign;
        lat2 *= latsign;

        let (mut sbet1, mut cbet1) = sincosdx(lat1);
        sbet1 *= self.f1;
        norm2(&mut sbet1, &mut cbet1);
        cbet1 = TINY.max(cbet1);

        let (mut sbet2, mut cbet2) = sincosdx(lat2);
        sbet2 *= self.f1;
        norm2(&mut sbet2, &mut cbet2);
        cbet2 = TINY.max(cbet2);

        if cbet1 < -sbet1 {
            if cbet2 == cbet1 {
                sbet2 = sbet1.copysign(sbet2);
            }
        } else if sbet2.abs() == -sbet1 {
            cbet2 = cbet1;
        }

        let dn1 = (1.0 + self.ep2 * sq(sbet1)).sqrt();
        let dn2 = (1.0 + self.ep2 * sq(sbet2)).sqrt();

        let mut meridian_solution = None;
        if lat1 == -QD || slam12 == 0.0 {
            let calp1 = clam12;
            let salp1 = slam12;
            let calp2 = 1.0;
            let salp2 = 0.0;

            let ssig1 = sbet1;
            let csig1 = calp1 * cbet1;
            let ssig2 = sbet2;
            let csig2 = calp2 * cbet2;

            let sig12 = (0.0_f64.max(csig1 * ssig2 - ssig1 * csig2) + 0.0)
                .atan2(csig1 * csig2 + ssig1 * ssig2);
            let lengths = distance_and_reduced_length(
                self.n,
                sig12,
                &SigmaEndpoints {
                    ssig1,
                    csig1,
                    dn1,
                    ssig2,
                    csig2,
                    dn2,
                },
            );
            let mut s12x = lengths.s12b;
            let m12x = lengths.m12b;
            // geodesic.c drops to the general solution for prolate lines close to antipodal, where m12 < 0
            if sig12 < TOL2 || m12x >= 0.0 {
                if sig12 < 3.0 * TINY || (sig12 < TOL0 && (s12x < 0.0 || m12x < 0.0)) {
                    s12x = 0.0;
                }
                meridian_solution = Some(InverseSolution {
                    salp1,
                    calp1,
                    salp2,
                    calp2,
                    s12x: s12x * self.b,
                });
            }
        }

        let solution = match meridian_solution {
            Some(solution) => solution,
            None if sbet1 == 0.0 && (self.f <= 0.0 || lon12s >= self.f * HD) => InverseSolution {
                salp1: 1.0,
                calp1: 0.0,
                salp2: 1.0,
                calp2: 0.0,
                s12x: self.a * lam12,
            },
            None => {
                let points = ReducedLatitudes {
                    sbet1,
                    cbet1,
                    dn1,
                    sbet2,
                    cbet2,
                    dn2,
                };
                let start = self.inverse_start(&points, lam12, slam12, clam12);
                if start.sig12 >= 0.0 {
                    InverseSolution {
                        salp1: start.salp1,
                        calp1: start.calp1,
                        salp2: start.salp2,
                        calp2: start.calp2,
                        s12x: start.sig12 * self.b * start.dnm,
                    }
                } else {
                    let mut salp1 = start.salp1;
                    let mut calp1 = start.calp1;
                    let mut numit: u32 = 0;
                    let (mut salp1a, mut calp1a, mut salp1b, mut calp1b) = (TINY, 1.0, TINY, -1.0);
                    let mut tripn = false;
                    let mut tripb = false;
                    let lambda = loop {
                        let lambda = self.lambda12(&points, salp1, calp1, slam12, clam12);
                        let v = lambda.lam12;
                        let tolerance = (if tripn { 8.0 } else { 1.0 }) * TOL0;
                        // geodesic.c tests !(fabs(v) >= tolerance) so a NaN ends the iteration
                        if tripb || v.abs() < tolerance || v.is_nan() || numit == MAXIT2 {
                            break lambda;
                        }
                        // geodesic.c Lambda12 returns a zero derivative from iteration MAXIT1 on
                        let dv = if numit < MAXIT1 {
                            self.lambda12_derivative(&points, &lambda)
                        } else {
                            0.0
                        };
                        if v > 0.0 && (numit > MAXIT1 || calp1 / salp1 > calp1b / salp1b) {
                            salp1b = salp1;
                            calp1b = calp1;
                        } else if v < 0.0 && (numit > MAXIT1 || calp1 / salp1 < calp1a / salp1a) {
                            salp1a = salp1;
                            calp1a = calp1;
                        }
                        if numit < MAXIT1 && dv > 0.0 {
                            let dalp1 = -v / dv;
                            if dalp1.abs() < PI {
                                let sdalp1 = dalp1.sin();
                                let cdalp1 = dalp1.cos();
                                let nsalp1 = salp1 * cdalp1 + calp1 * sdalp1;
                                if nsalp1 > 0.0 {
                                    calp1 = calp1 * cdalp1 - salp1 * sdalp1;
                                    salp1 = nsalp1;
                                    norm2(&mut salp1, &mut calp1);
                                    tripn = v.abs() <= 16.0 * TOL0;
                                    numit += 1;
                                    continue;
                                }
                            }
                        }
                        salp1 = (salp1a + salp1b) / 2.0;
                        calp1 = (calp1a + calp1b) / 2.0;
                        norm2(&mut salp1, &mut calp1);
                        tripn = false;
                        tripb = (salp1a - salp1).abs() + (calp1a - calp1) < TOLB
                            || (salp1 - salp1b).abs() + (calp1 - calp1b) < TOLB;
                        numit += 1;
                    };
                    let lengths = distance_and_reduced_length(
                        lambda.eps,
                        lambda.sig12,
                        &SigmaEndpoints {
                            ssig1: lambda.ssig1,
                            csig1: lambda.csig1,
                            dn1,
                            ssig2: lambda.ssig2,
                            csig2: lambda.csig2,
                            dn2,
                        },
                    );
                    InverseSolution {
                        salp1,
                        calp1,
                        salp2: lambda.salp2,
                        calp2: lambda.calp2,
                        s12x: lengths.s12b * self.b,
                    }
                }
            }
        };

        let s12 = 0.0 + solution.s12x;
        let (salp1, calp1) = if swapp < 0.0 {
            (solution.salp2, solution.calp2)
        } else {
            (solution.salp1, solution.calp1)
        };

        GeodesicInverse {
            s12,
            azi1: atan2dx(salp1 * (swapp * lonsign), calp1 * (swapp * latsign)),
        }
    }

    pub(super) fn direct(&self, lat1: f64, lon1: f64, azi1: f64, s12: f64) -> GeodesicDirect {
        // geod_direct builds a geodesic line with the C1, C1p and C3 capabilities and evaluates it at s12
        let azi1 = ang_normalize(azi1);
        let (salp1, calp1) = sincosdx(ang_round(azi1));

        let lat1 = lat_fix(lat1);
        let (mut sbet1, mut cbet1) = sincosdx(ang_round(lat1));
        sbet1 *= self.f1;
        norm2(&mut sbet1, &mut cbet1);
        cbet1 = TINY.max(cbet1);

        let salp0 = salp1 * cbet1;
        let calp0 = calp1.hypot(salp1 * sbet1);
        let mut ssig1 = sbet1;
        let somg1 = salp0 * sbet1;
        let mut csig1 = if sbet1 != 0.0 || calp1 != 0.0 {
            cbet1 * calp1
        } else {
            1.0
        };
        let comg1 = csig1;
        norm2(&mut ssig1, &mut csig1);

        let k2 = sq(calp0) * self.ep2;
        let eps = k2 / (2.0 * (1.0 + (1.0 + k2).sqrt()) + k2);

        let a1m1 = a1m1f(eps);
        let c1a = c1f(eps);
        let b11 = sin_series(ssig1, csig1, &c1a, ORDER);
        let s = b11.sin();
        let c = b11.cos();
        let stau1 = ssig1 * c + csig1 * s;
        let ctau1 = csig1 * c - ssig1 * s;

        let c1pa = c1pf(eps);

        let c3a = self.c3f(eps);
        let a3c = -self.f * salp0 * self.a3f(eps);
        let b31 = sin_series(ssig1, csig1, &c3a, ORDER - 1);

        let tau12 = s12 / (self.b * (1.0 + a1m1));
        let s = tau12.sin();
        let c = tau12.cos();
        let b12 = -sin_series(stau1 * c + ctau1 * s, ctau1 * c - stau1 * s, &c1pa, ORDER);
        let mut sig12 = tau12 - (b12 - b11);
        let mut ssig12 = sig12.sin();
        let mut csig12 = sig12.cos();
        if self.f.abs() > 0.01 {
            let ssig2 = ssig1 * csig12 + csig1 * ssig12;
            let csig2 = csig1 * csig12 - ssig1 * ssig12;
            let b12 = sin_series(ssig2, csig2, &c1a, ORDER);
            let serr = (1.0 + a1m1) * (sig12 + (b12 - b11)) - s12 / self.b;
            sig12 -= serr / (1.0 + k2 * sq(ssig2)).sqrt();
            ssig12 = sig12.sin();
            csig12 = sig12.cos();
        }

        let ssig2 = ssig1 * csig12 + csig1 * ssig12;
        let mut csig2 = csig1 * csig12 - ssig1 * ssig12;
        let sbet2 = calp0 * ssig2;
        let mut cbet2 = salp0.hypot(calp0 * csig2);
        if cbet2 == 0.0 {
            cbet2 = TINY;
            csig2 = TINY;
        }

        let somg2 = salp0 * ssig2;
        let comg2 = csig2;
        let omg12 = (somg2 * comg1 - comg2 * somg1).atan2(comg2 * comg1 + somg2 * somg1);
        let lam12 = omg12 + a3c * (sig12 + (sin_series(ssig2, csig2, &c3a, ORDER - 1) - b31));
        let lon12 = lam12 / DEGREE;
        let lon2 = ang_normalize(ang_normalize(lon1) + ang_normalize(lon12));
        let lat2 = atan2dx(sbet2, self.f1 * cbet2);

        GeodesicDirect { lat2, lon2 }
    }
}
