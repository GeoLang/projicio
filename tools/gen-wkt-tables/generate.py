# writes the wkt module's method mapping tables from PROJ's sources and its esri name table from proj.db
# usage: uv run tools/gen-wkt-tables/generate.py <PROJ 9.8.1 source checkout> <proj.db>
import re
import sqlite3
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RUST_OUT = REPO / "crates/projicio-core/src/iso19111/wkt/mappings.rs"
TSV_OUT = REPO / "crates/projicio-core/data/esri_names.tsv"

TOKEN = re.compile(
    r'\s+|//[^\n]*|/\*.*?\*/|"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z0-9_]*|-?[0-9]+(?:\.[0-9]+)?|::|.',
    re.S,
)


def tokens(text):
    text = text.replace("\\\n", " ")
    out = []
    for match in TOKEN.finditer(text):
        token = match.group(0)
        if token.isspace() or token.startswith("//") or token.startswith("/*"):
            continue
        if token.startswith('"') and out and out[-1].startswith('"'):
            out[-1] = out[-1][:-1] + token[1:]
            continue
        out.append(token)
    return out


def string_value(token):
    assert token.startswith('"'), token
    return token[1:-1].replace('\\"', '"').replace("\\\\", "\\")


def read_constants(proj_constants_h):
    constants = {}
    for line in proj_constants_h.replace("\\\n", " ").splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and parts[0] == "#define":
            value = tokens(parts[2])
            if len(value) == 1 and value[0].startswith('"'):
                constants[parts[1]] = string_value(value[0])
            elif len(value) == 1 and re.fullmatch(r"-?[0-9]+", value[0]):
                constants[parts[1]] = int(value[0])
    return constants


class Parser:
    def __init__(self, toks, constants):
        self.toks = toks
        self.pos = 0
        self.constants = constants

    def peek(self, offset=0):
        return self.toks[self.pos + offset]

    def next(self):
        token = self.toks[self.pos]
        self.pos += 1
        return token

    def expect(self, token):
        actual = self.next()
        assert actual == token, (token, actual, self.toks[self.pos - 5 : self.pos + 5])

    def value(self):
        token = self.next()
        if token == "{":
            items = []
            while self.peek() != "}":
                items.append(self.value())
                if self.peek() == ",":
                    self.next()
            self.next()
            return items
        if token == "&":
            return ("ref", self.next())
        if token.startswith('"'):
            return string_value(token)
        if re.fullmatch(r"-?[0-9]+", token):
            return int(token)
        if token in ("nullptr", "NULL"):
            return None
        if token in ("true", "false"):
            return token == "true"
        if token == "common":
            parts = [token]
            while self.peek() == "::":
                self.next()
                parts.append(self.next())
            return ("unit", parts[-1])
        if token in ("METHOD_NAME_CODE", "PARAM_NAME_CODE"):
            self.expect("(")
            suffix = self.next()
            self.expect(")")
            kind = "METHOD" if token == "METHOD_NAME_CODE" else "PARAMETER"
            return [
                self.constants[f"EPSG_NAME_{kind}_{suffix}"],
                self.constants[f"EPSG_CODE_{kind}_{suffix}"],
            ]
        if token in self.constants:
            return self.constants[token]
        if token.startswith("params") or token.endswith("Parameters"):
            return ("ident", token)
        raise ValueError(f"unknown token {token}")


def read_definitions(cpp, constants, struct_names):
    toks = tokens(cpp)
    definitions = {}
    order = []
    index = 0
    while index < len(toks):
        if toks[index] == "char" and toks[index + 1] == "*" and toks[index + 3] == "=":
            name = toks[index + 2]
            if toks[index + 4].startswith('"'):
                constants[name] = string_value(toks[index + 4])
            index += 5
            continue
        if toks[index] in struct_names:
            struct = toks[index]
            cursor = index + 1
            pointer_array = False
            if toks[cursor] == "*":
                pointer_array = True
                cursor += 1
                if toks[cursor] == "const":
                    cursor += 1
            name = toks[cursor]
            cursor += 1
            is_array = toks[cursor] == "["
            if is_array:
                cursor += 2
            if toks[cursor] == "=":
                cursor += 1
            if toks[cursor] != "{":
                index += 1
                continue
            parser = Parser(toks, constants)
            parser.pos = cursor
            definitions[name] = (struct, pointer_array, is_array, parser.value())
            order.append(name)
            index = parser.pos
            continue
        index += 1
    return definitions, order


def rust_str(value):
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def rust_option(value):
    return "None" if value is None else f"Some({rust_str(value)})"


def rust_ident(name):
    snake = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).upper()
    return re.sub(r"_+", "_", snake)


def generate_rust(proj_source):
    constants = read_constants((proj_source / "src/proj_constants.h").read_text())
    operation_dir = proj_source / "src/iso19111/operation"
    definitions, order = read_definitions(
        (operation_dir / "parammappings.cpp").read_text(),
        constants,
        {"ParamMapping", "MethodMapping", "MethodNameCode", "ParamNameCode"},
    )
    esri_definitions, esri_order = read_definitions(
        (operation_dir / "esriparammappings.cpp").read_text(),
        constants,
        {"ESRIParamMapping", "ESRIMethodMapping"},
    )

    lines = [
        "// generated by tools/gen-wkt-tables/generate.py from PROJ src/iso19111/operation/parammappings.cpp and esriparammappings.cpp (MIT), see LICENSES/PROJ-MIT.txt",
        "",
        "pub(crate) struct ParamMapping {",
        "    pub(crate) wkt2_name: &'static str,",
        "    pub(crate) epsg_code: i32,",
        "    pub(crate) wkt1_name: Option<&'static str>,",
        "}",
        "",
        "pub(crate) struct MethodMapping {",
        "    pub(crate) wkt2_name: &'static str,",
        "    pub(crate) epsg_code: i32,",
        "    pub(crate) wkt1_name: Option<&'static str>,",
        "    pub(crate) params: &'static [&'static ParamMapping],",
        "}",
        "",
        "pub(crate) struct EsriParamMapping {",
        "    pub(crate) esri_name: &'static str,",
        "    pub(crate) wkt2_name: Option<&'static str>,",
        "    pub(crate) epsg_code: i32,",
        "    pub(crate) fixed_value: &'static str,",
        "    pub(crate) is_fixed_value: bool,",
        "}",
        "",
        "pub(crate) struct EsriMethodMapping {",
        "    pub(crate) esri_name: &'static str,",
        "    pub(crate) wkt2_name: &'static str,",
        "    pub(crate) epsg_code: i32,",
        "    pub(crate) params: &'static [EsriParamMapping],",
        "}",
        "",
    ]

    def method_mapping(entry):
        wkt2_name, epsg_code, wkt1_name, _main, _aux, params = entry
        params_text = "&[]" if params is None else f"&{rust_ident(params[1])}"
        return (
            f"    MethodMapping {{ wkt2_name: {rust_str(wkt2_name)}, epsg_code: {epsg_code}, "
            f"wkt1_name: {rust_option(wkt1_name)}, params: {params_text} }},"
        )

    for name in order:
        struct, _pointer_array, is_array, value = definitions[name]
        if struct == "ParamMapping" and not is_array:
            wkt2_name, epsg_code, wkt1_name, _unit, _proj_name = value
            lines.append(
                f"static {rust_ident(name)}: ParamMapping = ParamMapping {{ wkt2_name: {rust_str(wkt2_name)}, "
                f"epsg_code: {epsg_code}, wkt1_name: {rust_option(wkt1_name)} }};"
            )
        elif struct == "ParamMapping":
            refs = [item[1] for item in value if item is not None]
            lines.append(
                f"static {rust_ident(name)}: [&ParamMapping; {len(refs)}] = [{', '.join('&' + rust_ident(ref) for ref in refs)}];"
            )
    lines.append("")
    for table_name, rust_name in (
        ("gProjectionMethodMappings", "PROJECTION_METHOD_MAPPINGS"),
        ("gOtherMethodMappings", "OTHER_METHOD_MAPPINGS"),
    ):
        entries = definitions[table_name][3]
        lines.append(f"pub(crate) static {rust_name}: [MethodMapping; {len(entries)}] = [")
        lines.extend(method_mapping(entry) for entry in entries)
        lines.append("];")
        lines.append("")
    for table_name, rust_name in (
        ("methodNameCodesList", "METHOD_NAME_CODES"),
        ("gParamNameCodes", "PARAM_NAME_CODES"),
    ):
        entries = definitions[table_name][3]
        lines.append(f"pub(crate) static {rust_name}: [(&str, i32); {len(entries)}] = [")
        lines.extend(f"    ({rust_str(entry[0])}, {entry[1]})," for entry in entries)
        lines.append("];")
        lines.append("")

    for name in esri_order:
        struct, _pointer_array, _is_array, value = esri_definitions[name]
        if struct != "ESRIParamMapping":
            continue
        entries = [entry for entry in value if entry[0] is not None]
        lines.append(f"pub(crate) static {rust_ident(name)}: [EsriParamMapping; {len(entries)}] = [")
        for esri_name, wkt2_name, epsg_code, fixed_value, is_fixed_value in entries:
            lines.append(
                f"    EsriParamMapping {{ esri_name: {rust_str(esri_name)}, wkt2_name: {rust_option(wkt2_name)}, "
                f"epsg_code: {epsg_code}, fixed_value: {rust_str(fixed_value)}, is_fixed_value: {str(is_fixed_value).lower()} }},"
            )
        lines.append("];")
    lines.append("")
    entries = esri_definitions["esriMappings"][3]
    lines.append(f"pub(crate) static ESRI_METHOD_MAPPINGS: [EsriMethodMapping; {len(entries)}] = [")
    for esri_name, wkt2_name, epsg_code, params in entries:
        lines.append(
            f"    EsriMethodMapping {{ esri_name: {rust_str(esri_name)}, wkt2_name: {rust_str(wkt2_name)}, "
            f"epsg_code: {epsg_code}, params: &{rust_ident(params[1])} }},"
        )
    lines.append("];")
    RUST_OUT.write_text("\n".join(lines) + "\n")


GENUINE_TABLE = {"geographic_2D_crs": "geodetic_crs", "geographic_3D_crs": "geodetic_crs"}


def genuine(table):
    return GENUINE_TABLE.get(table, table)


def type_filter(table):
    if table in ("geodetic_crs", "geographic_2D_crs"):
        return " AND type = 'geographic 2D'"
    if table == "geographic_3D_crs":
        return " AND type = 'geographic 3D'"
    return ""


def unique_esri_alias(aliases):
    # DatabaseContext getUniqueEsriAlias
    first, second = aliases[0], aliases[1]
    if "[" in second:
        first, second = second, first
    if first.replace("[", "").replace("]", "").replace("-", "_") == second:
        return first
    return ""


def alias_from_official_name(db, official_name, table, source):
    # DatabaseContext::getAliasFromOfficialName
    rows = db.execute(
        f'SELECT auth_name, code FROM "{genuine(table)}" WHERE name = ?{type_filter(table)} ORDER BY deprecated',
        (official_name,),
    ).fetchall()
    if not rows and not (official_name == "NAD83" and table == "geographic_3D_crs"):
        rows = db.execute(
            "SELECT auth_name, code FROM alias_name WHERE table_name = ? AND alt_name = ? AND source IN ('EPSG', 'PROJ')",
            (genuine(table), official_name),
        ).fetchall()
        if len(rows) != 1:
            return ""
    for auth_name, code in rows:
        aliases = [
            row[0]
            for row in db.execute(
                "SELECT alt_name FROM alias_name WHERE table_name = ? AND auth_name = ? AND code = ? AND source = ?",
                (genuine(table), auth_name, code, source),
            )
        ]
        if aliases:
            if len(aliases) == 2 and source == "ESRI":
                unique = unique_esri_alias(aliases)
                if unique:
                    return unique
            return aliases[0]
    return ""


def aliases_of_code(db, auth_name, code, table, source):
    # DatabaseContext::getAliases with an authority and code
    aliases = [
        row[0]
        for row in db.execute(
            "SELECT alt_name FROM alias_name WHERE table_name = ? AND auth_name = ? AND code = ? AND source = ?",
            (genuine(table), auth_name, code, source),
        )
    ]
    if len(aliases) == 2 and source == "ESRI":
        unique = unique_esri_alias(aliases)
        if unique:
            aliases = [unique]
    return aliases


def official_name_from_alias(db, alias, table):
    # AuthorityFactory::getOfficialNameFromAlias without equivalent spelling, source ESRI
    rows = db.execute(
        "SELECT table_name, auth_name, code FROM alias_name WHERE alt_name = ? AND table_name = ? AND source IN ('ESRI', 'ESRI_OLD')",
        (alias, table),
    ).fetchall()
    if not rows:
        return None
    candidates = []
    for table_name, auth_name, code in rows:
        candidates.extend(
            db.execute(
                f'SELECT name, ?, auth_name, code, deprecated FROM "{table_name}" WHERE auth_name = ? AND code = ?',
                (table_name, auth_name, code),
            ).fetchall()
        )
    if not candidates:
        return None
    # sqlite ORDER BY deprecated LIMIT 1 keeps the union order among equal keys
    candidates.sort(key=lambda row: row[4])
    return candidates[0]


def esri_object_names(db, table, type_constraint):
    # AuthorityFactory::createObjectsFromName on the ESRI authority without approximate match
    where = f" AND type = '{type_constraint}'" if type_constraint else ""
    names = {}
    for code, name in db.execute(f"SELECT code, name FROM {table} WHERE auth_name = 'ESRI'{where}"):
        names.setdefault(name.lower(), set()).add(code)
    for code, name in db.execute(
        f"SELECT ov.code, a.alt_name FROM {table} ov JOIN alias_name a ON ov.auth_name = a.auth_name AND ov.code = a.code "
        f"WHERE a.source != 'EPSG_OLD' AND a.table_name = '{table}' AND ov.auth_name = 'ESRI'{where.replace('type', 'ov.type')}"
    ):
        names.setdefault(name.lower(), set()).add(code)
    return sorted(name for name, codes in names.items() if len(codes) == 1)


def epsg_old_fallback(db, code, table):
    # the EPSG_OLD branch of GeodeticCRS::_exportToWKT and GeodeticReferenceFrame::_exportToWKT
    old_aliases = aliases_of_code(db, "EPSG", code, table, "EPSG_OLD")
    if len(old_aliases) != 1:
        return ""
    epsg_name = old_aliases[0]
    if table == "geodetic_datum":
        if epsg_name == "European Terrestrial Reference System 1989":
            epsg_name += " ensemble"
        candidates = db.execute(
            "SELECT auth_name, code FROM geodetic_datum WHERE auth_name = 'EPSG' AND name = ? COLLATE NOCASE "
            "ORDER BY deprecated, length(name), name",
            (epsg_name,),
        ).fetchall()
    else:
        nature = "geographic 3D" if table == "geographic_3D_crs" else "geographic 2D"
        candidates = db.execute(
            "SELECT auth_name, code FROM geodetic_crs WHERE auth_name = 'EPSG' AND name = ? COLLATE NOCASE AND type = ? "
            "ORDER BY deprecated, length(name), name",
            (epsg_name, nature),
        ).fetchall()
    for auth_name, candidate_code in candidates:
        esri_aliases = aliases_of_code(db, auth_name, candidate_code, table, "ESRI")
        if len(esri_aliases) == 1:
            return esri_aliases[0]
    return ""


OBJECT_TABLES = {
    "geographic_2D_crs": ("geodetic_crs", " AND type = 'geographic 2D'"),
    "geographic_3D_crs": ("geodetic_crs", " AND type = 'geographic 3D'"),
    "geocentric_crs": ("geodetic_crs", " AND type NOT IN ('geographic 2D', 'geographic 3D')"),
    "projected_crs": ("projected_crs", ""),
    "geodetic_datum": ("geodetic_datum", ""),
    "ellipsoid": ("ellipsoid", ""),
    "prime_meridian": ("prime_meridian", ""),
    "unit_of_measure": ("unit_of_measure", ""),
}


def object_rows(db):
    # objects with an ESRI alias, in the order the wkt module derives its lookups from, first one wins
    rows = []
    for table, (genuine_table, where) in OBJECT_TABLES.items():
        objects = db.execute(
            f'SELECT o.auth_name, o.code, o.name FROM "{genuine_table}" o WHERE EXISTS '
            f"(SELECT 1 FROM alias_name a WHERE a.table_name = ? AND a.auth_name = o.auth_name AND a.code = o.code "
            f"AND a.source IN ('ESRI', 'ESRI_OLD')){where} ORDER BY o.deprecated, o.auth_name, o.code",
            (genuine_table,),
        ).fetchall()
        for auth_name, code, name in objects:
            aliases = aliases_of_code(db, auth_name, code, genuine_table, "ESRI")
            rows.append((table, f"{auth_name}:{code}", name, aliases[0] if len(aliases) == 1 else ""))
    return rows


def geodetic_datums_named(db, name):
    # AuthorityFactory::createObjectsFromName(name, {GEODETIC_REFERENCE_FRAME}, false) on every authority, in its order
    rows = db.execute(
        "SELECT auth_name, code, name, deprecated, is_alias FROM ("
        "SELECT auth_name, code, name, deprecated, 0 AS is_alias FROM geodetic_datum WHERE name = ? COLLATE NOCASE "
        "UNION SELECT ov.auth_name, ov.code, a.alt_name, ov.deprecated, 1 FROM geodetic_datum ov JOIN alias_name a "
        "ON ov.auth_name = a.auth_name AND ov.code = a.code WHERE a.source != 'EPSG_OLD' "
        "AND a.table_name = 'geodetic_datum' AND a.alt_name = ? COLLATE NOCASE"
        ") ORDER BY deprecated, is_alias, length(name), name, auth_name, code",
        (name, name),
    ).fetchall()
    return [f"{auth_name}:{code}" for auth_name, code, *_ in rows]


def esri_vertical_rows(db):
    # exportAsESRIWktCompoundCRSWithEllipsoidalHeight: the datum named by a geodetic datum's ESRI name, and the ESRI
    # vertical CRS on that datum when there is exactly one, named as the ESRI branch of VerticalCRS::_exportToWKT names it
    vertical_by_datum = {}
    for name, datum_code, cs_auth_name, cs_code, vertical_datum_name in db.execute(
        "SELECT c.name, c.datum_code, c.coordinate_system_auth_name, c.coordinate_system_code, d.name "
        "FROM vertical_crs c JOIN vertical_datum d ON d.auth_name = c.datum_auth_name AND d.code = c.datum_code "
        "WHERE c.datum_auth_name = 'ESRI' AND c.datum_code LIKE 'from_geogdatum_%' AND c.deprecated = 0"
    ):
        assert (cs_auth_name, cs_code) == ("ESRI", "ELLPS_HEIGHT_METRE"), name
        vertical_by_datum.setdefault(datum_code, []).append((name, vertical_datum_name))
    esri_vertical_names = set(esri_object_names(db, "vertical_crs", None))

    official_names = {row[0] for row in db.execute("SELECT name FROM geodetic_datum WHERE auth_name = 'EPSG'")}
    official_names |= {
        row[0]
        for row in db.execute(
            "SELECT alt_name FROM alias_name WHERE table_name = 'geodetic_datum' AND source IN ('EPSG', 'PROJ')"
        )
    }
    searched_names = {alias_from_official_name(db, name, "geodetic_datum", "ESRI") or name for name in official_names}
    rows = []
    for searched_name in sorted(searched_names):
        datums = geodetic_datums_named(db, searched_name)
        if not datums:
            continue
        datum = datums[0]
        verticals = vertical_by_datum.get("from_geogdatum_" + datum.replace(":", "_"), [])
        vertical_name = ""
        if len(verticals) == 1:
            name, vertical_datum_name = verticals[0]
            assert geodetic_datums_named(db, vertical_datum_name)[:1] == [datum], (name, vertical_datum_name)
            vertical_name = alias_from_official_name(db, name, "vertical_crs", "ESRI")
            if not vertical_name:
                assert name.lower() in esri_vertical_names, f"{name} needs morphNameToESRI"
                vertical_name = name
        rows.append(("esri_vertical", searched_name, datum, vertical_name))
    return rows


def derived_by_name(objects, tables):
    derived = {}
    for table, _code, name, alias in objects:
        if table in tables and alias and name not in derived:
            derived[name] = alias
    return derived


def derived_official(objects, tables):
    derived = {}
    for table, code, name, alias in objects:
        if table in tables and alias and alias not in derived:
            derived[alias] = (name, code)
    return derived


def generate_tsv(proj_db):
    db = sqlite3.connect(f"file:{proj_db}?mode=ro", uri=True)
    objects = object_rows(db)
    rows = [("object",) + row for row in objects]

    by_name_tables = {
        "geographic_2D_crs": {"geographic_2D_crs"},
        "geographic_3D_crs": {"geographic_3D_crs"},
        "projected_crs": {"projected_crs"},
        "geodetic_datum": {"geodetic_datum"},
        "ellipsoid": {"ellipsoid"},
        "prime_meridian": {"prime_meridian"},
    }
    for table, object_tables in by_name_tables.items():
        derived = derived_by_name(objects, object_tables)
        names = {row[0] for row in db.execute(f'SELECT name FROM "{genuine(table)}" WHERE 1 = 1{type_filter(table)}')}
        # projected_crs keeps the 4863 EPSG short names out, 417 kB for names no WKT carries
        if table != "projected_crs":
            names |= {
                row[0]
                for row in db.execute(
                    "SELECT alt_name FROM alias_name WHERE table_name = ? AND source IN ('EPSG', 'PROJ')",
                    (genuine(table),),
                )
            }
        names |= set(derived)
        for name in sorted(names):
            alias = alias_from_official_name(db, name, table, "ESRI")
            if derived.get(name, "") != alias:
                rows.append(("by_name", table, name, alias))

    for table, type_constraint in (
        ("geodetic_crs", None),
        ("projected_crs", None),
        ("geodetic_datum", None),
        ("ellipsoid", None),
        ("prime_meridian", None),
    ):
        for name in esri_object_names(db, table, type_constraint):
            rows.append(("esri_object", table, name, ""))

    for table in ("geographic_2D_crs", "geographic_3D_crs", "geodetic_datum"):
        codes = db.execute(
            "SELECT DISTINCT code FROM alias_name WHERE table_name = ? AND auth_name = 'EPSG' AND source = 'EPSG_OLD'",
            (genuine(table),),
        ).fetchall()
        for (code,) in codes:
            alias = epsg_old_fallback(db, code, table)
            if alias:
                rows.append(("epsg_old", table, f"EPSG:{code}", alias))

    official_tables = {
        "geodetic_crs": {"geographic_2D_crs", "geographic_3D_crs", "geocentric_crs"},
        "projected_crs": {"projected_crs"},
        "geodetic_datum": {"geodetic_datum"},
        "ellipsoid": {"ellipsoid"},
        "prime_meridian": {"prime_meridian"},
        "unit_of_measure": {"unit_of_measure"},
    }
    for table, object_tables in official_tables.items():
        derived = derived_official(objects, object_tables)
        aliases = {
            row[0]
            for row in db.execute(
                "SELECT alt_name FROM alias_name WHERE table_name = ? AND source IN ('ESRI', 'ESRI_OLD')", (table,)
            )
        }
        for alias in sorted(aliases | set(derived)):
            found = official_name_from_alias(db, alias, table)
            expected = (found[0], f"{found[2]}:{found[3]}") if found else None
            if derived.get(alias) != expected:
                value = f"{expected[0]}\t{expected[1]}" if expected else "\t"
                rows.append(("official", table, alias, value))

    for (name,) in db.execute("SELECT name FROM geodetic_datum WHERE name LIKE '% (%)'"):
        rows.append(("datum_name", "geodetic_datum", name, ""))

    esri_names = {row[4] for row in rows if row[0] == "object" and row[1] == "projected_crs" and row[4]}
    esri_names |= {row[3] for row in rows if row[0] == "by_name" and row[1] == "projected_crs" and row[3]}
    for esri_name in sorted(esri_names):
        matches = db.execute(
            "SELECT code, text_definition FROM projected_crs WHERE auth_name = 'ESRI' AND name = ? COLLATE NOCASE",
            (esri_name,),
        ).fetchall()
        if len(matches) == 1 and (matches[0][1] or "").startswith("PROJCS"):
            rows.append(("esri_definition", "projected_crs", esri_name, matches[0][1]))

    rows.extend(esri_vertical_rows(db))

    for row in rows:
        assert not any("\n" in field for field in row), row
        assert not any("\t" in field for field in row[:3]), row
    TSV_OUT.write_text("".join("\t".join(row) + "\n" for row in rows))


def main():
    proj_source = Path(sys.argv[1])
    proj_db = Path(sys.argv[2])
    generate_rust(proj_source)
    generate_tsv(proj_db)


main()
