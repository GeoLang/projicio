import argparse
import math
import re
import sqlite3
import statistics
import subprocess
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

METRES_PER_DEGREE = 111_320.0
POINT_FRACTIONS = [(0.5, 0.5), (0.25, 0.25), (0.25, 0.75), (0.75, 0.25), (0.75, 0.75)]
THRESHOLDS_METRES = [0.001, 0.01, 0.1, 1.0]
AGREEING_BANDS = ("within 0.001 m", "within 0.01 m")
OPERATION_TABLES = {
    "helmert_transformation": "helmert",
    "grid_transformation": "grid",
    "other_transformation": "other",
    "concatenated_operation": "concatenated",
}


@dataclass
class CrsInfo:
    code: int
    name: str
    kind: str
    method: str
    datum: str
    axis_orientations: list[str]
    unit_factor: float


@dataclass
class Case:
    label: str
    source: CrsInfo
    target: CrsInfo
    points: list[tuple[float, float]]
    group: str
    proj_operation: str
    time_dependent: bool


@dataclass
class CaseResult:
    case: Case
    max_error_metres: float | None
    projicio_error: str | None
    proj_failed_points: int


def largest_extent(connection, table, code):
    rows = connection.execute(
        "SELECT e.west_lon, e.south_lat, e.east_lon, e.north_lat FROM usage u "
        "JOIN extent e ON e.auth_name = u.extent_auth_name AND e.code = u.extent_code "
        "WHERE u.object_table_name = ? AND u.object_auth_name = 'EPSG' AND u.object_code = ?",
        (table, code),
    ).fetchall()
    extents = []
    for west, south, east, north in rows:
        # proj.db stores extents crossing the antimeridian with west greater than east
        if west > east:
            east += 360.0
        extents.append((west, south, east, north))
    return max(extents, key=lambda e: (e[2] - e[0]) * (e[3] - e[1]), default=None)


def sample_points(extent):
    west, south, east, north = extent
    points = []
    for fx, fy in POINT_FRACTIONS:
        lon = west + fx * (east - west)
        if lon > 180.0:
            lon -= 360.0
        points.append((lon, south + fy * (north - south)))
    return points


def load_crs(connection):
    rows = connection.execute(
        "SELECT p.code, p.name, 'projected', c.method_name, d.name, p.coordinate_system_code "
        "FROM projected_crs p "
        "JOIN conversion c ON c.auth_name = p.conversion_auth_name AND c.code = p.conversion_code "
        "JOIN geodetic_crs g ON g.auth_name = p.geodetic_crs_auth_name AND g.code = p.geodetic_crs_code "
        "JOIN geodetic_datum d ON d.auth_name = g.datum_auth_name AND d.code = g.datum_code "
        "WHERE p.auth_name = 'EPSG' AND p.deprecated = 0 "
        "UNION ALL "
        "SELECT g.code, g.name, 'geographic', '', d.name, g.coordinate_system_code "
        "FROM geodetic_crs g "
        "JOIN geodetic_datum d ON d.auth_name = g.datum_auth_name AND d.code = g.datum_code "
        "WHERE g.auth_name = 'EPSG' AND g.deprecated = 0 AND g.type = 'geographic 2D'"
    ).fetchall()
    crs = {}
    for code, name, kind, method, datum, coordinate_system in rows:
        axes = connection.execute(
            "SELECT a.orientation, u.conv_factor FROM axis a "
            "JOIN unit_of_measure u ON u.auth_name = a.uom_auth_name AND u.code = a.uom_code "
            "WHERE a.coordinate_system_auth_name = 'EPSG' AND a.coordinate_system_code = ? "
            "ORDER BY a.coordinate_system_order",
            (coordinate_system,),
        ).fetchall()
        crs[int(code)] = CrsInfo(
            code=int(code),
            name=name,
            kind=kind,
            method=method,
            datum=datum,
            axis_orientations=[orientation for orientation, _ in axes],
            unit_factor=axes[0][1],
        )
    return crs


def time_dependent_names(connection):
    return {
        name
        for (name,) in connection.execute(
            "SELECT name FROM helmert_transformation WHERE auth_name = 'EPSG' "
            "AND (method_name LIKE 'Time-%' OR rate_tx IS NOT NULL OR epoch IS NOT NULL)"
        )
    }


def operation_kinds_by_name(connection):
    kinds = {}
    for table, kind in OPERATION_TABLES.items():
        for (name,) in connection.execute(f"SELECT name FROM {table} WHERE auth_name = 'EPSG'"):
            kinds.setdefault(name, kind)
    return kinds


def from_wgs84_cases(connection, crs):
    cases = []
    for info in crs.values():
        table = "projected_crs" if info.kind == "projected" else "geodetic_crs"
        extent = largest_extent(connection, table, info.code)
        if extent is None:
            continue
        cases.append(
            Case(
                label=f"EPSG:4326 to EPSG:{info.code}",
                source=crs[4326],
                target=info,
                points=sample_points(extent),
                group=info.datum,
                proj_operation="",
                time_dependent=False,
            )
        )
    return cases


def operation_pair_cases(connection, crs, time_dependent):
    cases = []
    skipped = 0
    for table, kind in OPERATION_TABLES.items():
        rows = connection.execute(
            f"SELECT code, name, source_crs_code, target_crs_code FROM {table} "
            "WHERE auth_name = 'EPSG' AND deprecated = 0 "
            "AND source_crs_auth_name = 'EPSG' AND target_crs_auth_name = 'EPSG'"
        ).fetchall()
        for code, name, source, target in rows:
            source_info, target_info = crs.get(int(source)), crs.get(int(target))
            extent = largest_extent(connection, table, code)
            # cs2cs and projicio transform two coordinates, so pairs touching a 3D or geocentric crs are left out
            if source_info is None or target_info is None or source_info.kind != "geographic" or target_info.kind != "geographic" or extent is None:
                skipped += 1
                continue
            cases.append(
                Case(
                    label=f"EPSG:{code} {name}",
                    source=source_info,
                    target=target_info,
                    points=sample_points(extent),
                    group=kind,
                    proj_operation=f"EPSG:{code}, {name}",
                    time_dependent=name in time_dependent,
                )
            )
    return cases, skipped


def northing_first(info):
    return info.axis_orientations[0] in ("north", "south")


def cs2cs_input(info, lon, lat):
    # cs2cs reads geographic input in the crs's own angular unit, a grad crs takes grads
    units_per_degree = math.radians(1.0) / info.unit_factor
    lon, lat = lon * units_per_degree, lat * units_per_degree
    return (lat, lon) if northing_first(info) else (lon, lat)


def proj_transform(case):
    lines = "".join("{!r} {!r}\n".format(*cs2cs_input(case.source, lon, lat)) for lon, lat in case.points)
    completed = subprocess.run(
        ["cs2cs", "-d", "12", f"EPSG:{case.source.code}", f"EPSG:{case.target.code}"],
        input=lines,
        capture_output=True,
        text=True,
    )
    # cs2cs exits non-zero when proj builds no operation at all for the pair
    if completed.returncode != 0:
        return [None] * len(case.points)
    results = []
    for line in completed.stdout.splitlines():
        first, second = line.split()[:2]
        if "*" in first or "inf" in first:
            results.append(None)
            continue
        # both sides keep south and west oriented axes signed as the crs defines them, only the order differs
        a, b = float(first), float(second)
        results.append((b, a) if northing_first(case.target) else (a, b))
    return results


def proj_best_operation(case):
    lons = [lon for lon, _ in case.points]
    lats = [lat for _, lat in case.points]
    bbox = f"{min(lons)},{min(lats)},{max(lons)},{max(lats)}"
    output = subprocess.run(
        [
            "projinfo", "-s", f"EPSG:{case.source.code}", "-t", f"EPSG:{case.target.code}",
            "--bbox", bbox, "--spatial-test", "intersects",
            "--grid-check", "discard_missing", "--summary",
        ],
        capture_output=True,
        text=True,
    )
    if output.returncode != 0:
        return f"projinfo failed: {output.stderr.strip()[:200]}"
    candidates = [line for line in output.stdout.splitlines() if line and not line.startswith("Candidate")]
    return candidates[0] if candidates else "none"


def projicio_transform(binary, cases):
    lines = "".join(
        f"EPSG:{case.source.code} EPSG:{case.target.code} {lon!r} {lat!r}\n"
        for case in cases
        for lon, lat in case.points
    )
    output = iter(
        subprocess.run([binary], input=lines, capture_output=True, text=True, check=True).stdout.splitlines()
    )
    return [[next(output) for _ in case.points] for case in cases]


def error_metres(target, lat, proj_xy, projicio_xy):
    if target.kind == "projected":
        return math.hypot(proj_xy[0] - projicio_xy[0], proj_xy[1] - projicio_xy[1]) * target.unit_factor
    # cs2cs prints geographic output in degrees even for a crs whose unit is the grad
    d_lon = (proj_xy[0] - projicio_xy[0] + 180.0) % 360.0 - 180.0
    d_lat = proj_xy[1] - projicio_xy[1]
    return math.hypot(d_lat * METRES_PER_DEGREE, d_lon * METRES_PER_DEGREE * math.cos(math.radians(lat)))


def compare_case(case, projicio_lines):
    proj_points = proj_transform(case)
    if not case.proj_operation:
        case.proj_operation = proj_best_operation(case)
    errors = []
    failed = 0
    for (_, lat), proj_xy, line in zip(case.points, proj_points, projicio_lines, strict=True):
        if line.startswith("error "):
            return CaseResult(case, None, line.removeprefix("error "), failed)
        if proj_xy is None:
            failed += 1
            continue
        projicio_xy = tuple(float(v) for v in line.split())
        errors.append(error_metres(case.target, lat, proj_xy, projicio_xy))
    return CaseResult(case, max(errors, default=None), None, failed)


def band(result):
    if result.projicio_error is not None:
        return "projicio refuses"
    if result.max_error_metres is None:
        return "proj fails"
    for threshold in THRESHOLDS_METRES:
        if result.max_error_metres <= threshold:
            return f"within {threshold:g} m"
    return f"over {THRESHOLDS_METRES[-1]:g} m"


def datum_step_kind(operation, kinds_by_name):
    # projinfo summary lines read "<id>, <name>, <accuracy>, <area>", a name joins steps with " + "
    parts = operation.split(", ", 2)
    if len(parts) < 2:
        return "unknown"
    steps = [re.sub(r"^Inverse of ", "", step) for step in parts[1].split(" + ")]
    found = {kinds_by_name.get(step) for step in steps}
    if any("allpark" in step for step in steps):
        return "ballpark"
    for kind in ("grid", "helmert", "other", "concatenated"):
        if kind in found:
            return kind
    return "no datum step"


def write_tsv(path, results):
    with path.open("w") as f:
        f.write("case\tsource\ttarget\ttarget_name\tgroup\tband\tmax_error_m\tproj_failed_points\ttime_dependent\tprojicio_error\tproj_operation\n")
        for r in results:
            error = "" if r.max_error_metres is None else f"{r.max_error_metres:.6g}"
            f.write(
                f"{r.case.label}\t{r.case.source.code}\t{r.case.target.code}\t{r.case.target.name}\t{r.case.group}\t"
                f"{band(r)}\t{error}\t{r.proj_failed_points}\t{r.case.time_dependent}\t{r.projicio_error or ''}\t{r.case.proj_operation}\n"
            )


def group_table(results, key, title, limit):
    groups = defaultdict(list)
    for r in results:
        groups[key(r)].append(r)

    def disagreeing(rs):
        return [r for r in rs if band(r) not in AGREEING_BANDS]

    ranked = sorted(groups.items(), key=lambda item: len(disagreeing(item[1])), reverse=True)
    lines = [f"\n{title}, by cases over 1 cm or refused (top {limit})", "over_1cm_or_refused\tcases\trefused\tmedian_error_m\tname"]
    for name, rs in ranked[:limit]:
        bad = disagreeing(rs)
        if not bad:
            break
        measured = [r.max_error_metres for r in rs if r.max_error_metres is not None]
        median = f"{statistics.median(measured):.3g}" if measured else "-"
        refused = sum(1 for r in rs if r.projicio_error is not None)
        lines.append(f"{len(bad)}\t{len(rs)}\t{refused}\t{median}\t{name or '(geographic)'}")
    return lines


def score_lines(results, title, kinds_by_name):
    lines = [f"\n== {title}: {len(results)} cases"]
    counts = defaultdict(int)
    for r in results:
        counts[band(r)] += 1
    order = [f"within {t:g} m" for t in THRESHOLDS_METRES] + [f"over {THRESHOLDS_METRES[-1]:g} m", "projicio refuses", "proj fails"]
    cumulative = 0
    for name in order:
        cumulative += counts[name] if name.startswith("within") else 0
        share = 100.0 * counts[name] / len(results)
        suffix = f", cumulative {100.0 * cumulative / len(results):.1f}%" if name.startswith("within") else ""
        lines.append(f"{name}: {counts[name]} ({share:.1f}%{suffix})")
    disagreeing = [r for r in results if band(r) not in AGREEING_BANDS]
    by_kind = defaultdict(int)
    for r in disagreeing:
        by_kind[datum_step_kind(r.case.proj_operation, kinds_by_name)] += 1
    lines.append("over 1 cm or refused, by the datum step proj ranks best: " + ", ".join(f"{k} {n}" for k, n in sorted(by_kind.items(), key=lambda i: -i[1])))
    lines.append(f"time-dependent operations among those: {sum(1 for r in disagreeing if r.case.time_dependent)}")
    refusals = defaultdict(int)
    for r in results:
        if r.projicio_error is not None:
            refusals[re.sub(r"EPSG:\d+", "EPSG:n", r.projicio_error)[:120]] += 1
    lines.append("projicio refusals, by message")
    lines += [f"{n}\t{message}" for message, n in sorted(refusals.items(), key=lambda i: -i[1])[:10]]
    return lines


def summary(from_wgs84, pairs, skipped_pairs, proj_release, epsg_version, kinds_by_name):
    lines = [f"{proj_release}, EPSG {epsg_version}"]
    lines.append("each case counts once, its error is the largest over 5 points inside the area of use")
    lines += score_lines(from_wgs84, "from EPSG:4326 to every current EPSG geographic 2D and projected crs", kinds_by_name)
    lines += group_table(from_wgs84, lambda r: r.case.target.datum, "datums", 40)
    lines += group_table(from_wgs84, lambda r: r.case.target.method, "projection methods", 25)
    lines += score_lines(pairs, f"every current EPSG operation between geographic 2D crs, source to target ({skipped_pairs} touching 3D or geocentric crs left out)", kinds_by_name)
    lines += group_table(pairs, lambda r: r.case.group, "operation kinds", 10)
    lines += group_table(pairs, lambda r: r.case.source.datum, "source datums", 30)
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--projicio-bin", required=True)
    parser.add_argument("--proj-db", default="/usr/share/proj/proj.db")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()

    connection = sqlite3.connect(f"file:{args.proj_db}?mode=ro", uri=True)
    crs = load_crs(connection)
    from_wgs84 = from_wgs84_cases(connection, crs)
    pairs, skipped_pairs = operation_pair_cases(connection, crs, time_dependent_names(connection))
    kinds_by_name = operation_kinds_by_name(connection)
    epsg_version = connection.execute("SELECT value FROM metadata WHERE key = 'EPSG.VERSION'").fetchone()[0]
    # cs2cs with no arguments prints its release line and usage to stderr
    proj_release = subprocess.run(["cs2cs"], capture_output=True, text=True).stderr.splitlines()[0]

    cases = from_wgs84 + pairs
    projicio_lines = projicio_transform(args.projicio_bin, cases)
    with ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(compare_case, cases, projicio_lines))
    from_wgs84_results, pair_results = results[: len(from_wgs84)], results[len(from_wgs84) :]

    args.out.mkdir(parents=True, exist_ok=True)
    write_tsv(args.out / "from-wgs84.tsv", from_wgs84_results)
    write_tsv(args.out / "pairs.tsv", pair_results)
    text = summary(from_wgs84_results, pair_results, skipped_pairs, proj_release, epsg_version, kinds_by_name)
    (args.out / "summary.txt").write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
