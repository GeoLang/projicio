//! Binary layout of projicio's embedded EPSG metadata registry
//! (`projicio-core/data/epsg.bin`).
//!
//! This crate is the single source of truth for the container shared by the
//! `projicio-core` reader and the `gen-epsg-registry` writer. Any layout or
//! semantic change is a format change: bump [`VERSION`] and regenerate the
//! artifact so the CI reproducibility check proves writer and reader agree.
//!
//! The registry carries metadata only: CRS names, kind, deprecation, datum
//! links, areas of use, and datum aliases. It never carries transform
//! behavior.
//!
//! # Container
//!
//! Little-endian throughout. Strings are `u16` byte-length-prefixed UTF-8.
//!
//! ```text
//! 0   magic          [u8; 8]   "EPSGMETA"
//! 8   format version u16
//! 10  section count  u16
//! 12  section table  section count * 16 bytes
//! ..  section payloads, in section table order
//! ```
//!
//! Each section table entry is `tag: u32`, `record_count: u32`,
//! `byte_offset: u32` (from the start of the file), `byte_len: u32`.
//!
//! A reader looks sections up by tag and ignores tags it does not know, so
//! adding a section is a data change rather than a format break.
//!
//! # Section tags
//!
//! Assigned:
//!
//! - [`SECTION_CRS`] — one record per EPSG CRS
//! - [`SECTION_EXTENT`] — one record per referenced area of use
//! - [`SECTION_DATUM_ALIAS`] — datum code to name or alternative name
//!
//! Reserved for sections this registry does not carry yet. Nothing may reuse
//! these values for another purpose:
//!
//! - [`SECTION_COORDINATE_OPERATION`] — operation records
//! - [`SECTION_GRID_RESOURCE`] — datum shift grid records
//! - [`SECTION_DATUM`] — datum definitions
//! - [`SECTION_ELLIPSOID`] — ellipsoid definitions
//!
//! Tags 8 through 15 are reserved and unassigned. Tags 16 and above are free
//! for future use.
//!
//! # Records
//!
//! CRS record, [`CRS_RECORD_BASE_SIZE`] bytes then the name string:
//!
//! ```text
//! 0   code        u32
//! 4   kind        u8   CRS_KIND_*
//! 5   flags       u8   FLAG_*
//! 6   datum code  u32  NO_CODE when the CRS has no datum link
//! 10  extent code u32  NO_CODE when the CRS has no area of use
//! 14  name        string
//! ```
//!
//! Extent record, [`EXTENT_RECORD_BASE_SIZE`] bytes then the name string:
//!
//! ```text
//! 0   code   u32
//! 4   west   f64   degrees
//! 12  south  f64   degrees
//! 20  east   f64   degrees
//! 28  north  f64   degrees
//! 36  name   string
//! ```
//!
//! An extent whose `west` exceeds its `east` crosses the antimeridian.
//!
//! Datum alias record, [`DATUM_ALIAS_RECORD_BASE_SIZE`] bytes then the name
//! string:
//!
//! ```text
//! 0   datum code  u32
//! 4   name        string
//! ```

#![forbid(unsafe_code)]

/// File magic, the first eight bytes of the container.
pub const MAGIC: [u8; 8] = *b"EPSGMETA";
/// Container format version. Bump on any layout or semantic change.
pub const VERSION: u16 = 1;
/// Magic (8) + format version (2) + section count (2).
pub const HEADER_SIZE: usize = 12;
/// Tag (4) + record count (4) + byte offset (4) + byte length (4).
pub const SECTION_ENTRY_SIZE: usize = 16;

/// One record per EPSG CRS, sorted by code.
pub const SECTION_CRS: u32 = 1;
/// One record per area of use referenced by a CRS record, sorted by code.
pub const SECTION_EXTENT: u32 = 2;
/// Datum names and alternative names, sorted by datum code then name.
pub const SECTION_DATUM_ALIAS: u32 = 3;
/// Reserved for coordinate operation records.
pub const SECTION_COORDINATE_OPERATION: u32 = 4;
/// Reserved for datum shift grid records.
pub const SECTION_GRID_RESOURCE: u32 = 5;
/// Reserved for datum definition records.
pub const SECTION_DATUM: u32 = 6;
/// Reserved for ellipsoid definition records.
pub const SECTION_ELLIPSOID: u32 = 7;
/// Tags below this value are assigned or reserved by this crate.
pub const FIRST_UNRESERVED_SECTION_TAG: u32 = 16;

/// A CRS expressed in longitude and latitude.
pub const CRS_KIND_GEOGRAPHIC: u8 = 1;
/// A CRS expressed in projected plane coordinates.
pub const CRS_KIND_PROJECTED: u8 = 2;
/// Any other CRS kind, including vertical, compound, and engineering.
pub const CRS_KIND_OTHER: u8 = 3;

/// EPSG marks this CRS deprecated.
pub const FLAG_DEPRECATED: u8 = 1 << 0;

/// Stands in for an absent datum or extent link, since EPSG has no code zero.
pub const NO_CODE: u32 = 0;

/// Fixed part of a CRS record, excluding the trailing name string.
pub const CRS_RECORD_BASE_SIZE: usize = 14;
/// Fixed part of an extent record, excluding the trailing name string.
pub const EXTENT_RECORD_BASE_SIZE: usize = 36;
/// Fixed part of a datum alias record, excluding the trailing name string.
pub const DATUM_ALIAS_RECORD_BASE_SIZE: usize = 4;

/// Bounds-checked little-endian read primitives.
///
/// Every read returns `None` rather than panicking so a reader can reject a
/// truncated or corrupt blob with an error instead of aborting the process.
pub mod read {
    pub fn bytes(data: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
        data.get(offset..offset.checked_add(len)?)
    }

    pub fn u16(data: &[u8], offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(bytes(data, offset, 2)?.try_into().ok()?))
    }

    pub fn u32(data: &[u8], offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes(data, offset, 4)?.try_into().ok()?))
    }

    pub fn u8(data: &[u8], offset: usize) -> Option<u8> {
        data.get(offset).copied()
    }

    pub fn f64(data: &[u8], offset: usize) -> Option<f64> {
        Some(f64::from_le_bytes(bytes(data, offset, 8)?.try_into().ok()?))
    }

    /// Read a `u16` length-prefixed UTF-8 string, returning it with the total
    /// number of bytes it occupies including the prefix.
    pub fn string_u16(data: &[u8], offset: usize) -> Option<(&str, usize)> {
        let len = usize::from(u16(data, offset)?);
        let text = std::str::from_utf8(bytes(data, offset + 2, len)?).ok()?;
        Some((text, 2 + len))
    }
}

/// Little-endian write primitives.
///
/// Float canonicalization is generator policy and lives with the generator.
pub mod write {
    /// A string too long for the registry's `u16` byte-length prefix.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StringTooLongError {
        byte_len: usize,
    }

    impl StringTooLongError {
        pub fn byte_len(self) -> usize {
            self.byte_len
        }
    }

    impl std::fmt::Display for StringTooLongError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                formatter,
                "string is {} bytes, over the registry limit of {}",
                self.byte_len,
                u16::MAX
            )
        }
    }

    impl std::error::Error for StringTooLongError {}

    pub fn u8(buf: &mut Vec<u8>, value: u8) {
        buf.push(value);
    }

    pub fn u16(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    pub fn f64(buf: &mut Vec<u8>, value: f64) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a `u16` length-prefixed UTF-8 string, leaving `buf` untouched
    /// when the string does not fit.
    pub fn string_u16(buf: &mut Vec<u8>, value: &str) -> Result<(), StringTooLongError> {
        let text = value.as_bytes();
        let len = u16::try_from(text.len()).map_err(|_| StringTooLongError {
            byte_len: text.len(),
        })?;
        u16(buf, len);
        buf.extend_from_slice(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_roundtrip() {
        let mut buf = Vec::new();
        write::u8(&mut buf, 0x2A);
        write::u16(&mut buf, 0xBEEF);
        write::u32(&mut buf, 0xDEAD_BEEF);
        write::f64(&mut buf, -12.5);
        write::string_u16(&mut buf, "WGS 84").unwrap();

        assert_eq!(read::u8(&buf, 0), Some(0x2A));
        assert_eq!(read::u16(&buf, 1), Some(0xBEEF));
        assert_eq!(read::u32(&buf, 3), Some(0xDEAD_BEEF));
        assert_eq!(read::f64(&buf, 7), Some(-12.5));
        assert_eq!(read::string_u16(&buf, 15), Some(("WGS 84", 8)));
    }

    #[test]
    fn truncated_reads_report_none() {
        let mut buf = Vec::new();
        write::string_u16(&mut buf, "WGS 84").unwrap();
        buf.pop();

        assert_eq!(read::string_u16(&buf, 0), None);
        assert_eq!(read::u32(&buf, 5), None);
        assert_eq!(read::f64(&buf, 0), None);
        assert_eq!(read::u8(&buf, buf.len()), None);
    }

    #[test]
    fn invalid_utf8_string_is_rejected() {
        let mut buf = Vec::new();
        write::u16(&mut buf, 2);
        buf.extend_from_slice(&[0xFF, 0xFE]);

        assert_eq!(read::string_u16(&buf, 0), None);
    }

    #[test]
    fn oversized_string_write_leaves_buffer_untouched() {
        let mut buf = vec![1, 2, 3];
        let oversized = "x".repeat(usize::from(u16::MAX) + 1);

        let error = write::string_u16(&mut buf, &oversized).unwrap_err();

        assert_eq!(error.byte_len(), oversized.len());
        assert_eq!(buf, [1, 2, 3]);
    }

    #[test]
    fn section_tags_are_distinct_and_reserved() {
        let tags = [
            SECTION_CRS,
            SECTION_EXTENT,
            SECTION_DATUM_ALIAS,
            SECTION_COORDINATE_OPERATION,
            SECTION_GRID_RESOURCE,
            SECTION_DATUM,
            SECTION_ELLIPSOID,
        ];
        let mut distinct = tags.to_vec();
        distinct.sort_unstable();
        distinct.dedup();

        assert_eq!(distinct.len(), tags.len());
        assert!(tags.iter().all(|tag| *tag < FIRST_UNRESERVED_SECTION_TAG));
        assert!(tags.iter().all(|tag| *tag != NO_CODE));
    }
}
