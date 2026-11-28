import argparse
import math
import re
import sqlite3
import statistics
import subprocess
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from functools import cache
from pathlib import Path

METRES_PER_DEGREE = 111_320.0
POINT_FRACTIONS = [(0.5, 0.5), (0.25, 0.25), (0.25, 0.75), (0.75, 0.25), (0.75, 0.75)]
THRESHOLDS_METRES = [0.001, 0.01, 0.1, 1.0]
AGREEING_BANDS = ("within 0.001 m", "within 0.01 m")
PARITY_METRES = 0.01
OPERATION_TABLES = {
    "helmert_transformation": "helmert",
    "grid_transformation": "grid",
    "other_transformation": "other",
    "concatenated_operation": "concatenated",
}
CRS_TABLES = {
    "geographic 2D": "geodetic_crs",
    "geographic 3D": "geodetic_crs",
    "geocentric": "geodetic_crs",
    "projected": "projected_crs",
    "vertical": "vertical_crs",
    "compound": "compound_crs",
    "engineering": "engineering_crs",
}
# proj.db's latest frame reference epoch is 2024.0 and its latest Helmert reference epoch 2021.6164
COORDINATE_EPOCH = 2025.0
SOURCE_HEIGHT = 100.0


@dataclass
class CrsInfo:
    code: int
    name: str
    kind: str
    method: str
    datum: str
    axis_orientations: list[str]
    axis_unit_factors: list[float]
    dynamic: bool
    base: "CrsInfo | None"
    parts: "tuple[CrsInfo, CrsInfo] | None"
    ellipsoid_semi_axes: tuple[float, float] | None


@dataclass
class Case:
    label: str
    source: CrsInfo
    target: CrsInfo
    points: list[tuple[float, float]]
    group: str
    method: str
    proj_operation: str
    time_dependent: bool
    dimension: int
    epoch: float | None


@dataclass
class Disagreement:
    distance_metres: float
    horizontal_metres: float | None
    height_metres: float | None


@dataclass
class CaseResult:
    case: Case
    max_error_metres: float | None
    max_horizontal_metres: float | None
    max_height_metres: float | None
    projicio_error: str | None
    proj_failed_points: int
    compared_points: int
    projicio_refused_points: int
    projicio_answered_points: int
    parity_failure: str | None


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


def axes(connection, coordinate_system):
    rows = connection.execute(
        "SELECT a.orientation, u.conv_factor FROM axis a "
        "JOIN unit_of_measure u ON u.auth_name = a.uom_auth_name AND u.code = a.uom_code "
        "WHERE a.coordinate_system_auth_name = 'EPSG' AND a.coordinate_system_code = ? "
        "ORDER BY a.coordinate_system_order",
        (coordinate_system,),
    ).fetchall()
    return [orientation for orientation, _ in rows], [factor for _, factor in rows]


def load_crs(connection):
    geodetic = {}
    for code, name, kind, datum, coordinate_system, dynamic, semi_major, semi_minor, inverse_flattening in connection.execute(
        "SELECT g.code, g.name, g.type, d.name, g.coordinate_system_code, d.frame_reference_epoch IS NOT NULL, "
        "e.semi_major_axis * u.conv_factor, e.semi_minor_axis * u.conv_factor, e.inv_flattening "
        "FROM geodetic_crs g "
        "JOIN geodetic_datum d ON d.auth_name = g.datum_auth_name AND d.code = g.datum_code "
        "JOIN ellipsoid e ON e.auth_name = d.ellipsoid_auth_name AND e.code = d.ellipsoid_code "
        "JOIN unit_of_measure u ON u.auth_name = e.uom_auth_name AND u.code = e.uom_code "
        "WHERE g.auth_name = 'EPSG' AND g.deprecated = 0"
    ):
        if semi_minor is None:
            semi_minor = semi_major * (1.0 - 1.0 / inverse_flattening)
        orientations, factors = axes(connection, coordinate_system)
        geodetic[int(code)] = CrsInfo(
            code=int(code), name=name, kind=kind, method="", datum=datum,
            axis_orientations=orientations, axis_unit_factors=factors, dynamic=bool(dynamic),
            base=None, parts=None, ellipsoid_semi_axes=(semi_major, semi_minor),
        )

    crs = {}
    for code, name, method, datum, coordinate_system, base in connection.execute(
        "SELECT p.code, p.name, c.method_name, d.name, p.coordinate_system_code, p.geodetic_crs_code "
        "FROM projected_crs p "
        "JOIN conversion c ON c.auth_name = p.conversion_auth_name AND c.code = p.conversion_code "
        "JOIN geodetic_crs g ON g.auth_name = p.geodetic_crs_auth_name AND g.code = p.geodetic_crs_code "
        "JOIN geodetic_datum d ON d.auth_name = g.datum_auth_name AND d.code = g.datum_code "
        "WHERE p.auth_name = 'EPSG' AND p.deprecated = 0"
    ):
        orientations, factors = axes(connection, coordinate_system)
        base_info = geodetic[int(base)]
        crs[int(code)] = CrsInfo(
            code=int(code), name=name, kind="projected", method=method, datum=datum,
            axis_orientations=orientations, axis_unit_factors=factors, dynamic=base_info.dynamic,
            base=base_info, parts=None, ellipsoid_semi_axes=None,
        )
    crs.update(geodetic)

    for code, name, datum, coordinate_system, dynamic in connection.execute(
        "SELECT v.code, v.name, d.name, v.coordinate_system_code, d.frame_reference_epoch IS NOT NULL "
        "FROM vertical_crs v JOIN vertical_datum d ON d.auth_name = v.datum_auth_name AND d.code = v.datum_code "
        "WHERE v.auth_name = 'EPSG' AND v.deprecated = 0"
    ):
        orientations, factors = axes(connection, coordinate_system)
        crs[int(code)] = CrsInfo(
            code=int(code), name=name, kind="vertical", method="", datum=datum,
            axis_orientations=orientations, axis_unit_factors=factors, dynamic=bool(dynamic),
            base=None, parts=None, ellipsoid_semi_axes=None,
        )

    for code, name, datum, coordinate_system in connection.execute(
        "SELECT e.code, e.name, d.name, e.coordinate_system_code "
        "FROM engineering_crs e JOIN engineering_datum d ON d.auth_name = e.datum_auth_name AND d.code = e.datum_code "
        "WHERE e.auth_name = 'EPSG' AND e.deprecated = 0"
    ):
        orientations, factors = axes(connection, coordinate_system)
        crs[int(code)] = CrsInfo(
            code=int(code), name=name, kind="engineering", method="", datum=datum,
            axis_orientations=orientations, axis_unit_factors=factors, dynamic=False,
            base=None, parts=None, ellipsoid_semi_axes=None,
        )

    for code, name, horizontal_code, vertical_code in connection.execute(
        "SELECT code, name, horiz_crs_code, vertical_crs_code FROM compound_crs WHERE auth_name = 'EPSG' AND deprecated = 0"
    ):
        horizontal, vertical = crs[int(horizontal_code)], crs[int(vertical_code)]
        crs[int(code)] = CrsInfo(
            code=int(code), name=name, kind="compound", method=horizontal.method,
            datum=f"{horizontal.datum} + {vertical.datum}",
            axis_orientations=horizontal.axis_orientations + vertical.axis_orientations,
            axis_unit_factors=horizontal.axis_unit_factors + vertical.axis_unit_factors,
            dynamic=horizontal.dynamic or vertical.dynamic,
            base=None, parts=(horizontal, vertical), ellipsoid_semi_axes=None,
        )
    return crs


def horizontal_part(info):
    return info.parts[0] if info.parts else info


def has_height(info):
    return len(info.axis_orientations) == 3 or info.kind == "vertical"


def time_dependent_names(connection):
    return {
        name
        for (name,) in connection.execute(
            "SELECT name FROM helmert_transformation WHERE auth_name = 'EPSG' "
            "AND (method_name LIKE 'Time-%' OR rate_tx IS NOT NULL OR epoch IS NOT NULL)"
        )
    }


def time_dependent_codes(connection):
    codes = {
        int(code)
        for (code,) in connection.execute(
            "SELECT code FROM coordinate_operation_view WHERE auth_name = 'EPSG' "
            "AND (method_name LIKE 'Time-%' OR method_name LIKE '%velocit%' "
            "OR method_name LIKE 'Point motion%' OR method_name LIKE '%Deformation Model%') "
            "UNION SELECT code FROM helmert_transformation WHERE auth_name = 'EPSG' "
            "AND (rate_tx IS NOT NULL OR epoch IS NOT NULL)"
        )
    }
    for operation, step in connection.execute(
        "SELECT operation_code, step_code FROM concatenated_operation_step "
        "WHERE operation_auth_name = 'EPSG' AND step_auth_name = 'EPSG'"
    ):
        if int(step) in codes:
            codes.add(int(operation))
    return codes


def operation_kinds_by_name(connection):
    kinds = {}
    for table, kind in OPERATION_TABLES.items():
        for (name,) in connection.execute(f"SELECT name FROM {table} WHERE auth_name = 'EPSG'"):
            kinds.setdefault(name, kind)
    return kinds


def from_wgs84_cases(connection, crs):
    cases = []
    for info in crs.values():
        if info.kind not in ("geographic 2D", "projected"):
            continue
        extent = largest_extent(connection, CRS_TABLES[info.kind], info.code)
        if extent is None:
            continue
        cases.append(
            Case(
                label=f"EPSG:4326 to EPSG:{info.code}",
                source=crs[4326],
                target=info,
                points=sample_points(extent),
                group=info.datum,
                method=info.method,
                proj_operation="",
                time_dependent=False,
                dimension=2,
                epoch=None,
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
            if source_info is None or target_info is None or source_info.kind != "geographic 2D" or target_info.kind != "geographic 2D" or extent is None:
                skipped += 1
                continue
            cases.append(
                Case(
                    label=f"EPSG:{code} {name}",
                    source=source_info,
                    target=target_info,
                    points=sample_points(extent),
                    group=kind,
                    method=kind,
                    proj_operation=f"EPSG:{code}, {name}",
                    time_dependent=name in time_dependent,
                    dimension=2,
                    epoch=None,
                )
            )
    return cases, skipped


def crs_cases(connection, crs):
    cases = []
    for info in crs.values():
        extent = largest_extent(connection, CRS_TABLES[info.kind], info.code)
        if extent is None:
            raise ValueError(f"EPSG:{info.code} has no area of use in proj.db")
        source = crs[4979] if has_height(info) else crs[4326]
        epoch = COORDINATE_EPOCH if info.dynamic else None
        cases.append(
            Case(
                label=f"EPSG:{source.code} to EPSG:{info.code}",
                source=source,
                target=info,
                points=sample_points(extent),
                group=info.datum,
                method=info.method,
                proj_operation="",
                time_dependent=False,
                dimension=3 if has_height(info) or epoch is not None else 2,
                epoch=epoch,
            )
        )
    return cases


def operation_cases(connection, crs, time_dependent):
    cases = []
    for table, code, name, method, source, target in connection.execute(
        "SELECT table_name, code, name, method_name, source_crs_code, target_crs_code "
        "FROM coordinate_operation_view WHERE auth_name = 'EPSG' AND deprecated = 0"
    ).fetchall():
        source_info, target_info = crs[int(source)], crs[int(target)]
        extent = largest_extent(connection, table, code)
        if extent is None:
            raise ValueError(f"EPSG:{code} has no area of use in proj.db")
        epoch = COORDINATE_EPOCH if source_info.dynamic or target_info.dynamic or int(code) in time_dependent else None
        cases.append(
            Case(
                label=f"EPSG:{code} {name}",
                source=source_info,
                target=target_info,
                points=sample_points(extent),
                group=OPERATION_TABLES[table],
                method=method or "concatenated operation",
                proj_operation=f"EPSG:{code}, {name}",
                time_dependent=int(code) in time_dependent,
                dimension=3 if has_height(source_info) or has_height(target_info) or epoch is not None else 2,
                epoch=epoch,
            )
        )
    return cases


def northing_first(info):
    return info.axis_orientations[0] in ("north", "south")


def cs2cs_input(info, coordinate):
    first, second, *height = coordinate
    if info.kind == "vertical":
        # cs2cs input for a vertical crs goes to the operation unconverted, as longitude and latitude in radians
        return (math.radians(first), math.radians(second), *height)
    horizontal = horizontal_part(info)
    if horizontal.kind.startswith("geographic"):
        # cs2cs reads geographic input in the crs's own angular unit, a grad crs takes grads
        units_per_degree = math.radians(1.0) / horizontal.axis_unit_factors[0]
        first, second = first * units_per_degree, second * units_per_degree
    if northing_first(horizontal):
        first, second = second, first
    return (first, second, *height)


def cs2cs_answer(info, line, dimension):
    values = line.split()[:dimension]
    if any("*" in value or "inf" in value for value in values):
        return None
    first, second, *height = (float(value) for value in values)
    horizontal = horizontal_part(info)
    if info.parts and horizontal.kind.startswith("geographic"):
        # cs2cs output for a compound crs is in the crs's angular unit, not degrees
        degrees_per_unit = horizontal.axis_unit_factors[0] / math.radians(1.0)
        first, second = first * degrees_per_unit, second * degrees_per_unit
    # both sides keep south and west oriented axes signed as the crs defines them, only the order differs
    if northing_first(horizontal):
        first, second = second, first
    return (first, second, *height)


@cache
def run_cs2cs(source_code, target_code, lines):
    completed = subprocess.run(
        ["cs2cs", "-d", "12", f"EPSG:{source_code}", f"EPSG:{target_code}"],
        input=lines,
        capture_output=True,
        text=True,
    )
    # cs2cs exits non-zero when proj builds no operation at all for the pair
    if completed.returncode != 0:
        return None
    return tuple(completed.stdout.splitlines())


def proj_points(source, target, coordinates, epoch):
    present = [coordinate for coordinate in coordinates if coordinate is not None]
    if not present:
        return [None] * len(coordinates)
    # cs2cs's fourth input column is the coordinate epoch
    epoch_column = [] if epoch is None else [epoch]
    lines = "".join(
        " ".join(repr(value) for value in [*cs2cs_input(source, coordinate), *epoch_column]) + "\n"
        for coordinate in present
    )
    output = run_cs2cs(source.code, target.code, lines)
    if output is None:
        answers = iter([None] * len(present))
    else:
        answers = iter([cs2cs_answer(target, line, len(coordinate)) for line, coordinate in zip(output, present, strict=True)])
    return [None if coordinate is None else next(answers) for coordinate in coordinates]


def geocentric_coordinates(info, lon, lat, height):
    semi_major, semi_minor = info.ellipsoid_semi_axes
    eccentricity_squared = 1.0 - (semi_minor / semi_major) ** 2
    latitude, longitude = math.radians(lat), math.radians(lon)
    prime_vertical_radius = semi_major / math.sqrt(1.0 - eccentricity_squared * math.sin(latitude) ** 2)
    return (
        (prime_vertical_radius + height) * math.cos(latitude) * math.cos(longitude),
        (prime_vertical_radius + height) * math.cos(latitude) * math.sin(longitude),
        (prime_vertical_radius * (1.0 - eccentricity_squared) + height) * math.sin(latitude),
    )


def horizontal_coordinates(info, points):
    if info.kind == "projected":
        return proj_points(info.base, info, points, None)
    return list(points)


def source_coordinates(case):
    source = case.source
    if source.kind == "geocentric":
        return [geocentric_coordinates(source, lon, lat, SOURCE_HEIGHT) for lon, lat in case.points]
    if source.kind == "engineering":
        # proj.db has no geodetic base for engineering crs
        horizontal = proj_points(case.target, source, horizontal_coordinates(case.target, case.points), None)
    else:
        horizontal = horizontal_coordinates(horizontal_part(source), case.points)
    if case.dimension == 2:
        return horizontal
    # cs2cs takes a missing height as 0
    height = SOURCE_HEIGHT if has_height(source) else 0.0
    return [None if xy is None else (*xy, height) for xy in horizontal]


@cache
def proj_best_operation(source_code, target_code, bbox):
    output = subprocess.run(
        [
            "projinfo", "-s", f"EPSG:{source_code}", "-t", f"EPSG:{target_code}",
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


def batch_line(case, coordinate):
    values = [*coordinate] if case.epoch is None else [*coordinate, case.epoch]
    return f"EPSG:{case.source.code} EPSG:{case.target.code} " + " ".join(repr(value) for value in values) + "\n"


def projicio_transform(binary, cases, inputs_per_case):
    lines = "".join(
        batch_line(case, coordinate)
        for case, inputs in zip(cases, inputs_per_case, strict=True)
        for coordinate in inputs
        if coordinate is not None
    )
    output = iter(
        subprocess.run([binary], input=lines, capture_output=True, text=True, check=True).stdout.splitlines()
    )
    return [[None if coordinate is None else next(output) for coordinate in inputs] for inputs in inputs_per_case]


def horizontal_metres(info, lat, proj_xy, projicio_xy):
    if info.kind in ("projected", "engineering"):
        return math.hypot(proj_xy[0] - projicio_xy[0], proj_xy[1] - projicio_xy[1]) * info.axis_unit_factors[0]
    # cs2cs prints geographic output in degrees even for a crs whose unit is the grad
    d_lon = (proj_xy[0] - projicio_xy[0] + 180.0) % 360.0 - 180.0
    d_lat = proj_xy[1] - projicio_xy[1]
    return math.hypot(d_lat * METRES_PER_DEGREE, d_lon * METRES_PER_DEGREE * math.cos(math.radians(lat)))


def disagreement(target, lat, proj_answer, projicio_answer):
    if target.kind == "geocentric":
        return Disagreement(math.dist(proj_answer, projicio_answer), None, None)
    if target.kind == "vertical":
        height = abs(proj_answer[2] - projicio_answer[2]) * target.axis_unit_factors[0]
        return Disagreement(height, None, height)
    horizontal = horizontal_metres(horizontal_part(target), lat, proj_answer, projicio_answer)
    if len(proj_answer) < 3 or not has_height(target):
        return Disagreement(horizontal, horizontal, None)
    height = abs(proj_answer[2] - projicio_answer[2]) * target.axis_unit_factors[2]
    return Disagreement(math.hypot(horizontal, height), horizontal, height)


def over_parity_reason(target, max_horizontal, max_height):
    if target.kind == "geocentric":
        return "over 1 cm, geocentric distance"
    if max_height is not None and (max_horizontal is None or max_height > max_horizontal):
        return "over 1 cm, height"
    return "over 1 cm, horizontal"


def compare_case(case, inputs, projicio_lines):
    proj_answers = proj_points(case.source, case.target, inputs, case.epoch)
    if not case.proj_operation:
        lons = [lon for lon, _ in case.points]
        lats = [lat for _, lat in case.points]
        case.proj_operation = proj_best_operation(
            case.source.code, case.target.code, f"{min(lons)},{min(lats)},{max(lons)},{max(lats)}"
        )
    disagreements = []
    first_refusal = None
    proj_refused = projicio_refused = projicio_answered = 0
    for (_, lat), coordinate, proj_answer, line in zip(case.points, inputs, proj_answers, projicio_lines, strict=True):
        if coordinate is None:
            continue
        refused = line.startswith("error ")
        if refused and first_refusal is None:
            first_refusal = line.removeprefix("error ")
        if proj_answer is None:
            proj_refused += 1
            if not refused:
                projicio_answered += 1
            continue
        if refused:
            projicio_refused += 1
            continue
        projicio_answer = tuple(float(value) for value in line.split())
        if len(projicio_answer) != len(coordinate):
            raise ValueError(f"{case.label}: sent {len(coordinate)} coordinates, projicio answered {line!r}")
        disagreements.append(disagreement(case.target, lat, proj_answer, projicio_answer))

    max_distance = max((d.distance_metres for d in disagreements), default=None)
    max_horizontal = max((d.horizontal_metres for d in disagreements if d.horizontal_metres is not None), default=None)
    max_height = max((d.height_metres for d in disagreements if d.height_metres is not None), default=None)
    if all(coordinate is None for coordinate in inputs):
        failure = "no input point, cs2cs puts none in the source crs"
    elif projicio_refused:
        failure = "projicio refuses where PROJ answers"
    elif projicio_answered:
        failure = "projicio answers where PROJ refuses"
    elif max_distance is not None and max_distance > PARITY_METRES:
        failure = over_parity_reason(case.target, max_horizontal, max_height)
    else:
        failure = None
    return CaseResult(
        case=case,
        max_error_metres=max_distance,
        max_horizontal_metres=max_horizontal,
        max_height_metres=max_height,
        projicio_error=first_refusal,
        proj_failed_points=proj_refused,
        compared_points=len(disagreements),
        projicio_refused_points=projicio_refused,
        projicio_answered_points=projicio_answered,
        parity_failure=failure,
    )


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


def refusal_message(message):
    return re.sub(r"EPSG:\d+", "EPSG:n", message)[:120]


def number(value):
    return "" if value is None else f"{value:.6g}"


def write_tsv(path, results):
    with path.open("w") as f:
        f.write("case\tsource\ttarget\ttarget_name\tgroup\tband\tmax_error_m\tproj_failed_points\ttime_dependent\tprojicio_error\tproj_operation\n")
        for r in results:
            f.write(
                f"{r.case.label}\t{r.case.source.code}\t{r.case.target.code}\t{r.case.target.name}\t{r.case.group}\t"
                f"{band(r)}\t{number(r.max_error_metres)}\t{r.proj_failed_points}\t{r.case.time_dependent}\t{r.projicio_error or ''}\t{r.case.proj_operation}\n"
            )


def write_parity_tsv(path, results):
    with path.open("w") as f:
        f.write(
            "case\tsource\ttarget\tsource_kind\ttarget_kind\ttarget_name\tmethod\tgroup\tdimension\tepoch\t"
            "parity\tfailure\tmax_distance_m\tmax_horizontal_m\tmax_height_m\tcompared_points\tproj_refused_points\t"
            "projicio_refused_points\tprojicio_answered_points\ttime_dependent\tprojicio_error\tproj_operation\n"
        )
        for r in results:
            c = r.case
            f.write(
                f"{c.label}\t{c.source.code}\t{c.target.code}\t{c.source.kind}\t{c.target.kind}\t{c.target.name}\t{c.method}\t{c.group}\t"
                f"{c.dimension}\t{c.epoch or ''}\t{'pass' if r.parity_failure is None else 'fail'}\t{r.parity_failure or ''}\t"
                f"{number(r.max_error_metres)}\t{number(r.max_horizontal_metres)}\t{number(r.max_height_metres)}\t{r.compared_points}\t"
                f"{r.proj_failed_points}\t{r.projicio_refused_points}\t{r.projicio_answered_points}\t{c.time_dependent}\t"
                f"{r.projicio_error or ''}\t{c.proj_operation}\n"
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
            refusals[refusal_message(r.projicio_error)] += 1
    lines.append("projicio refusals, by message")
    lines += [f"{n}\t{message}" for message, n in sorted(refusals.items(), key=lambda i: -i[1])[:10]]
    return lines


def passing_share(results):
    passing = [r for r in results if r.parity_failure is None]
    both_refuse = sum(1 for r in passing if r.compared_points == 0)
    return f"{len(passing)} of {len(results)} pass ({100.0 * len(passing) / len(results):.1f}%), {both_refuse} of them because both refuse"


def parity_lines(results, title):
    lines = [f"\n== {title}: {passing_share(results)}"]
    lines.append("failing cases, by reason")
    reasons = Counter(r.parity_failure for r in results if r.parity_failure is not None)
    lines += [f"{n}\t{reason}" for reason, n in reasons.most_common()]
    lines.append("projicio refusals where PROJ answers, by message")
    refusals = Counter(refusal_message(r.projicio_error) for r in results if r.projicio_refused_points)
    lines += [f"{n}\t{message}" for message, n in refusals.most_common(10)]
    lines.append(f"time-dependent operations among the failing cases: {sum(1 for r in results if r.parity_failure and r.case.time_dependent)}")
    return lines


def parity_table(results, key, title, limit):
    groups = defaultdict(list)
    for r in results:
        groups[key(r)].append(r)
    ranked = sorted(groups.items(), key=lambda item: sum(1 for r in item[1] if r.parity_failure), reverse=True)
    lines = [
        f"\n{title}, by failing cases (top {limit})",
        "failing\tcases\tpassing\tboth_refuse\tprojicio_refuses\tprojicio_answers\tmedian_distance_m\tname",
    ]
    for name, rs in ranked[:limit]:
        passing = [r for r in rs if r.parity_failure is None]
        measured = [r.max_error_metres for r in rs if r.max_error_metres is not None]
        median = f"{statistics.median(measured):.3g}" if measured else "-"
        lines.append(
            f"{len(rs) - len(passing)}\t{len(rs)}\t{len(passing)}\t{sum(1 for r in passing if r.compared_points == 0)}\t"
            f"{sum(1 for r in rs if r.projicio_refused_points)}\t{sum(1 for r in rs if r.projicio_answered_points)}\t"
            f"{median}\t{name or '(none)'}"
        )
    return lines


def summary(scores, skipped_pairs, proj_release, epsg_version, kinds_by_name):
    crs_results, operation_results = scores["crs"], scores["operations"]
    with_epoch = sum(1 for r in crs_results + operation_results if r.case.epoch is not None)
    lines = [f"{proj_release}, EPSG {epsg_version}"]
    lines.append("each case counts once, its error is the largest over 5 points inside the area of use")
    lines.append(
        f"parity: every point within {PARITY_METRES:g} m of cs2cs, height counted where the target has one "
        "(geocentric: 3D distance, vertical: height only), or refused by both"
    )
    lines.append(
        f"coordinate epoch {COORDINATE_EPOCH} on the {with_epoch} cases with a dynamic crs or a time-dependent operation, "
        "given to cs2cs as the fourth input column"
    )
    lines.append(f"source height {SOURCE_HEIGHT:g} in the source crs's height unit, 0 where only the target has a height")
    lines.append(f"\n== full scope: {passing_share(crs_results + operation_results)}")

    lines += parity_lines(crs_results, "from EPSG:4326, or EPSG:4979 where the target has a height, to every current EPSG crs")
    lines += parity_table(crs_results, lambda r: r.case.target.kind, "crs kinds", 10)
    lines += parity_table(crs_results, lambda r: datum_step_kind(r.case.proj_operation, kinds_by_name), "datum steps proj ranks best", 10)
    lines += parity_table(crs_results, lambda r: r.case.target.datum, "datums", 40)
    lines += parity_table(crs_results, lambda r: r.case.method, "projection methods", 25)

    lines += parity_lines(operation_results, "every current EPSG operation, source crs to target crs")
    lines += parity_table(operation_results, lambda r: f"{r.case.source.kind} to {r.case.target.kind}", "crs kinds", 20)
    lines += parity_table(operation_results, lambda r: r.case.group, "operation kinds", 10)
    lines += parity_table(operation_results, lambda r: r.case.method, "operation methods", 40)

    from_wgs84, pairs = scores["from-wgs84"], scores["pairs"]
    lines += score_lines(from_wgs84, "sub-score: from EPSG:4326 to every current EPSG geographic 2D and projected crs", kinds_by_name)
    lines += group_table(from_wgs84, lambda r: r.case.target.datum, "datums", 40)
    lines += group_table(from_wgs84, lambda r: r.case.target.method, "projection methods", 25)
    lines += score_lines(pairs, f"sub-score: every current EPSG operation between geographic 2D crs, source to target ({skipped_pairs} touching other crs kinds left out)", kinds_by_name)
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
    pairs, skipped_pairs = operation_pair_cases(connection, crs, time_dependent_names(connection))
    case_sets = {
        "from-wgs84": from_wgs84_cases(connection, crs),
        "pairs": pairs,
        "crs": crs_cases(connection, crs),
        "operations": operation_cases(connection, crs, time_dependent_codes(connection)),
    }
    kinds_by_name = operation_kinds_by_name(connection)
    epsg_version = connection.execute("SELECT value FROM metadata WHERE key = 'EPSG.VERSION'").fetchone()[0]
    # cs2cs with no arguments prints its release line and usage to stderr
    proj_release = subprocess.run(["cs2cs"], capture_output=True, text=True).stderr.splitlines()[0]

    cases = [case for case_set in case_sets.values() for case in case_set]
    with ThreadPoolExecutor(args.jobs) as pool:
        inputs = list(pool.map(source_coordinates, cases))
    projicio_lines = projicio_transform(args.projicio_bin, cases, inputs)
    with ThreadPoolExecutor(args.jobs) as pool:
        results = iter(list(pool.map(compare_case, cases, inputs, projicio_lines)))
    scores = {name: [next(results) for _ in case_set] for name, case_set in case_sets.items()}

    args.out.mkdir(parents=True, exist_ok=True)
    write_tsv(args.out / "from-wgs84.tsv", scores["from-wgs84"])
    write_tsv(args.out / "pairs.tsv", scores["pairs"])
    write_parity_tsv(args.out / "crs.tsv", scores["crs"])
    write_parity_tsv(args.out / "operations.tsv", scores["operations"])
    text = summary(scores, skipped_pairs, proj_release, epsg_version, kinds_by_name)
    (args.out / "summary.txt").write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
