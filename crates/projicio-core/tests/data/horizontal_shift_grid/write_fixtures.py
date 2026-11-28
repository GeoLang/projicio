import math
import struct
import sys
from pathlib import Path


def ntv2_record(order, label, value):
    if isinstance(value, bytes):
        return label.ljust(8).encode() + value.ljust(8)
    if isinstance(value, int):
        return label.ljust(8).encode() + struct.pack(order + "i4x", value)
    return label.ljust(8).encode() + struct.pack(order + "d", value)


def ntv2_subgrid(name, parent, south, north, west, east, step, shift):
    # ntv2 counts longitude positive west, in arc-seconds, and stores each row east to west
    rows = round((north - south) / step) + 1
    columns = round((east - west) / step) + 1
    header = b"".join(
        [
            ntv2_record("<", "SUB_NAME", name.ljust(8).encode()),
            ntv2_record("<", "PARENT", parent.ljust(8).encode()),
            ntv2_record("<", "CREATED", b"20261127"),
            ntv2_record("<", "UPDATED", b"20261127"),
            ntv2_record("<", "S_LAT", south * 3600.0),
            ntv2_record("<", "N_LAT", north * 3600.0),
            ntv2_record("<", "E_LONG", -east * 3600.0),
            ntv2_record("<", "W_LONG", -west * 3600.0),
            ntv2_record("<", "LAT_INC", step * 3600.0),
            ntv2_record("<", "LONG_INC", step * 3600.0),
            ntv2_record("<", "GS_COUNT", rows * columns),
        ]
    )
    nodes = b""
    for row in range(rows):
        latitude = south + row * step
        for column in reversed(range(columns)):
            longitude = west + column * step
            latitude_shift, longitude_shift_east = shift(longitude, latitude)
            nodes += struct.pack("<4f", latitude_shift, -longitude_shift_east, 0.0, 0.0)
    return header + nodes


def parent_shift(longitude, latitude):
    return (
        1.5 + 0.4 * math.sin(math.radians(60.0 * longitude)) * latitude / 46.0,
        2.0 - 0.3 * math.cos(math.radians(80.0 * latitude)) + 0.05 * longitude * longitude,
    )


def child_shift(longitude, latitude):
    latitude_shift, longitude_shift = parent_shift(longitude, latitude)
    return (
        latitude_shift + 0.7 + 0.2 * math.sin(math.radians(150.0 * longitude)),
        longitude_shift - 0.6 + 0.15 * math.cos(math.radians(120.0 * latitude)),
    )


def ntv2_with_subgrid():
    overview = b"".join(
        [
            ntv2_record("<", "NUM_OREC", 11),
            ntv2_record("<", "NUM_SREC", 11),
            ntv2_record("<", "NUM_FILE", 2),
            ntv2_record("<", "GS_TYPE", b"SECONDS"),
            ntv2_record("<", "VERSION", b"NTv2.0"),
            ntv2_record("<", "SYSTEM_F", b"TEST"),
            ntv2_record("<", "SYSTEM_T", b"TEST"),
            ntv2_record("<", "MAJOR_F", 6378137.0),
            ntv2_record("<", "MINOR_F", 6356752.314),
            ntv2_record("<", "MAJOR_T", 6378137.0),
            ntv2_record("<", "MINOR_T", 6356752.314),
        ]
    )
    parent = ntv2_subgrid("PARENT", "NONE", 45.0, 47.0, 1.0, 4.0, 0.5, parent_shift)
    child = ntv2_subgrid("CHILD", "PARENT", 45.5, 46.5, 2.0, 3.0, 0.25, child_shift)
    return overview + parent + child + b"END     " + bytes(8)


def ntv1(south, north, west, east, step, shift):
    # ntv1 is big-endian, in degrees for the header and arc-seconds for the shifts
    def record(label, value):
        if isinstance(value, bytes):
            return label.ljust(8).encode() + value.ljust(8)
        return label.ljust(8).encode() + struct.pack(">d", value)

    header = b"".join(
        [
            b"HEADER  " + struct.pack(">i4x", 12),
            record("S LAT", south),
            record("N LAT", north),
            record("E LONG", -east),
            record("W LONG", -west),
            record("N GRID", step),
            record("W GRID", step),
            record("TYPE", b"SECONDS"),
            record("FROM", b"NAD27"),
            record("TO", b"NAD83"),
            bytes(32),
        ]
    )
    rows = round((north - south) / step) + 1
    columns = round((east - west) / step) + 1
    nodes = b""
    for row in range(rows):
        latitude = south + row * step
        for column in reversed(range(columns)):
            longitude = west + column * step
            latitude_shift, longitude_shift_east = shift(longitude, latitude)
            nodes += struct.pack(">2d", latitude_shift, -longitude_shift_east)
    return header + nodes


def main():
    directory = Path(sys.argv[1])
    (directory / "nonlinear_with_subgrid.gsb").write_bytes(ntv2_with_subgrid())
    (directory / "nonlinear_ntv1.dat").write_bytes(
        ntv1(49.0, 50.0, -96.0, -94.5, 0.25, parent_shift)
    )


main()
