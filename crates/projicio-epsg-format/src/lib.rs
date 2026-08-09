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

/// Checksum used by the registry provenance record.
///
/// It lives here so the generator that records a digest and the reader that
/// pins it cannot drift apart, and so neither needs a dependency for it.
pub mod checksum {
    const INITIAL_STATE: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    const ROUND_CONSTANTS: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    const BLOCK_SIZE: usize = 64;
    const LENGTH_SUFFIX_OFFSET: usize = 56;

    /// SHA-256 of `bytes`, lowercase hex with a `sha256:` prefix.
    pub fn sha256_hex(bytes: &[u8]) -> String {
        let mut block = Vec::with_capacity((bytes.len() + 72).div_ceil(BLOCK_SIZE) * BLOCK_SIZE);
        block.extend_from_slice(bytes);
        block.push(0x80);
        while block.len() % BLOCK_SIZE != LENGTH_SUFFIX_OFFSET {
            block.push(0);
        }
        block.extend_from_slice(&(bytes.len() as u64).wrapping_mul(8).to_be_bytes());

        let mut state = INITIAL_STATE;
        for chunk in block.chunks_exact(BLOCK_SIZE) {
            let mut schedule = [0u32; 64];
            for (target, word) in schedule.iter_mut().zip(chunk.chunks_exact(4)) {
                *target = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for index in 16..schedule.len() {
                let previous = schedule[index - 15];
                let recent = schedule[index - 2];
                let mixed_previous =
                    previous.rotate_right(7) ^ previous.rotate_right(18) ^ (previous >> 3);
                let mixed_recent =
                    recent.rotate_right(17) ^ recent.rotate_right(19) ^ (recent >> 10);
                schedule[index] = schedule[index - 16]
                    .wrapping_add(mixed_previous)
                    .wrapping_add(schedule[index - 7])
                    .wrapping_add(mixed_recent);
            }

            let mut working = state;
            for (constant, word) in ROUND_CONSTANTS.iter().zip(schedule.iter()) {
                let sum1 = working[4].rotate_right(6)
                    ^ working[4].rotate_right(11)
                    ^ working[4].rotate_right(25);
                let choose = (working[4] & working[5]) ^ (!working[4] & working[6]);
                let temp1 = working[7]
                    .wrapping_add(sum1)
                    .wrapping_add(choose)
                    .wrapping_add(*constant)
                    .wrapping_add(*word);
                let sum0 = working[0].rotate_right(2)
                    ^ working[0].rotate_right(13)
                    ^ working[0].rotate_right(22);
                let majority = (working[0] & working[1])
                    ^ (working[0] & working[2])
                    ^ (working[1] & working[2]);
                let temp2 = sum0.wrapping_add(majority);

                working[7] = working[6];
                working[6] = working[5];
                working[5] = working[4];
                working[4] = working[3].wrapping_add(temp1);
                working[3] = working[2];
                working[2] = working[1];
                working[1] = working[0];
                working[0] = temp1.wrapping_add(temp2);
            }

            for (total, value) in state.iter_mut().zip(working) {
                *total = total.wrapping_add(value);
            }
        }

        let mut hex = String::from("sha256:");
        for word in state {
            hex.push_str(&format!("{word:08x}"));
        }
        hex
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

    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            checksum::sha256_hex(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            checksum::sha256_hex(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            checksum::sha256_hex(&b"x".repeat(1000)),
            "sha256:44f8354494a5ba03ba1792a8d3e9c534c47a9181980fde7a3f44b06ef2ae7c7f"
        );
    }
}
