//! UI-independent SGF byte codec: CA-driven decoding and lossless encoding.
//!
//! This module owns the single encoding strategy shared by every client. The
//! frozen Tauri adapter and the GPUI client both delegate here, so a Shift_JIS
//! game file is decoded and re-encoded identically on either side.
//!
//! Rules:
//! - The `CA` property of the SGF tree selects the encoding; no `CA` means
//!   UTF-8 (the SGF default).
//! - Decoding never inserts replacement characters; bytes that cannot be
//!   strictly decoded are rejected.
//! - Encoding never drops characters; content that cannot be represented in
//!   the source encoding is rejected before any write starts.

use encoding_rs::{BIG5, EUC_JP, Encoding, GBK, SHIFT_JIS, UTF_8};
use thiserror::Error;

use crate::{DecodedGameFile, SourceEncoding};

/// Rejects invalid or lossy byte-level SGF content with a stable, readable
/// reason. Error text is user-facing and must stay stable.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum FileCodecError {
    #[error("the SGF is not valid UTF-8 and does not declare a supported CA encoding")]
    InvalidUtf8WithoutCa,
    #[error(
        "the SGF declares unsupported CA encoding {label:?}; supported encodings are UTF-8, Shift_JIS, EUC-JP, GBK, and Big5"
    )]
    UnsupportedEncoding { label: String },
    #[error("the SGF CA property has no closing bracket")]
    UnclosedCaProperty,
    #[error("the SGF CA property must contain a non-empty ASCII encoding label")]
    InvalidCaLabel,
    #[error("the SGF bytes cannot be decoded as {encoding} without replacement characters")]
    Undecodable { encoding: &'static str },
    #[error(
        "the edited SGF cannot be represented as {encoding} without data loss; remove unsupported characters before saving"
    )]
    NotLosslesslyEncodable { encoding: &'static str },
}

/// Decodes SGF bytes according to their declared `CA` encoding. A missing `CA`
/// defaults to UTF-8; a UTF-8 byte order mark is stripped. Invalid bytes or
/// unsupported declarations fail without touching any file.
pub fn decode_sgf_bytes(bytes: &[u8]) -> Result<DecodedGameFile, FileCodecError> {
    let encoding = detect_sgf_encoding(bytes)?.unwrap_or(SourceEncoding::Utf8);
    let content = match encoding {
        SourceEncoding::Utf8 => {
            let decoded =
                std::str::from_utf8(bytes).map_err(|_| FileCodecError::InvalidUtf8WithoutCa)?;
            decoded
                .strip_prefix('\u{feff}')
                .unwrap_or(decoded)
                .to_owned()
        }
        _ => encoding_rs_for(encoding)
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|decoded| decoded.into_owned())
            .ok_or(FileCodecError::Undecodable {
                encoding: encoding_label(encoding),
            })?,
    };

    Ok(DecodedGameFile { content, encoding })
}

/// Encodes SGF text back into the given source encoding. UTF-8 is written as
/// raw bytes; legacy encodings reject content that would need replacement
/// characters, so a save can never silently corrupt the file.
pub fn encode_sgf(content: &str, encoding: SourceEncoding) -> Result<Vec<u8>, FileCodecError> {
    if encoding == SourceEncoding::Utf8 {
        return Ok(content.as_bytes().to_vec());
    }

    let (encoded, _, had_replacements) = encoding_rs_for(encoding).encode(content);
    if had_replacements {
        return Err(FileCodecError::NotLosslesslyEncodable {
            encoding: encoding_label(encoding),
        });
    }
    Ok(encoded.into_owned())
}

/// Detects the encoding declared by the SGF `CA` property. `None` means no
/// declaration was found (the caller defaults to UTF-8); unsupported
/// declarations and malformed `CA` properties are errors.
pub fn detect_sgf_encoding(bytes: &[u8]) -> Result<Option<SourceEncoding>, FileCodecError> {
    let Some(declared_label) = find_declared_sgf_encoding(bytes)? else {
        return Ok(None);
    };

    match normalize_encoding_label(&declared_label).as_str() {
        "UTF8" => Ok(Some(SourceEncoding::Utf8)),
        "SHIFTJIS" | "SJIS" | "MSKANJI" => Ok(Some(SourceEncoding::ShiftJis)),
        "EUCJP" => Ok(Some(SourceEncoding::EucJp)),
        "GBK" | "GB2312" => Ok(Some(SourceEncoding::Gbk)),
        "BIG5" => Ok(Some(SourceEncoding::Big5)),
        _ => Err(FileCodecError::UnsupportedEncoding {
            label: declared_label,
        }),
    }
}

/// Scans the raw bytes for the first well-formed `CA[...]` property and returns
/// its declared label.
fn find_declared_sgf_encoding(bytes: &[u8]) -> Result<Option<String>, FileCodecError> {
    for byte_index in 0..bytes.len().saturating_sub(2) {
        if bytes.get(byte_index..byte_index + 3) != Some(b"CA[")
            || !is_sgf_property_boundary(bytes, byte_index)
        {
            continue;
        }
        let value_start = byte_index + 3;
        let Some(value_end_offset) = bytes[value_start..].iter().position(|byte| *byte == b']')
        else {
            return Err(FileCodecError::UnclosedCaProperty);
        };
        let value_end = value_start + value_end_offset;
        let declared_label = std::str::from_utf8(&bytes[value_start..value_end])
            .map_err(|_| FileCodecError::InvalidCaLabel)?;
        if declared_label.is_empty() || !declared_label.is_ascii() {
            return Err(FileCodecError::InvalidCaLabel);
        }
        return Ok(Some(declared_label.to_owned()));
    }

    Ok(None)
}

/// A `CA` property only counts when it starts a property name: at the very
/// beginning of the file or right after a property terminator.
fn is_sgf_property_boundary(bytes: &[u8], byte_index: usize) -> bool {
    byte_index == 0
        || matches!(
            bytes[byte_index - 1],
            b';' | b']' | b'(' | b'\n' | b'\r' | b'\t' | b' '
        )
}

fn normalize_encoding_label(label: &str) -> String {
    label
        .chars()
        .filter(|character| !matches!(character, '-' | '_' | ' '))
        .flat_map(char::to_uppercase)
        .collect()
}

fn encoding_rs_for(encoding: SourceEncoding) -> &'static Encoding {
    match encoding {
        SourceEncoding::Utf8 => UTF_8,
        SourceEncoding::ShiftJis => SHIFT_JIS,
        SourceEncoding::EucJp => EUC_JP,
        SourceEncoding::Gbk => GBK,
        SourceEncoding::Big5 => BIG5,
    }
}

fn encoding_label(encoding: SourceEncoding) -> &'static str {
    match encoding {
        SourceEncoding::Utf8 => "UTF-8",
        SourceEncoding::ShiftJis => "Shift_JIS",
        SourceEncoding::EucJp => "EUC-JP",
        SourceEncoding::Gbk => "GBK",
        SourceEncoding::Big5 => "Big5",
    }
}

#[cfg(test)]
mod tests {
    use super::{FileCodecError, decode_sgf_bytes, detect_sgf_encoding, encode_sgf};
    use crate::SourceEncoding;

    #[test]
    fn decodes_shift_jis_sgf_from_its_ca_declaration() {
        let source_sgf = "(;FF[4]CA[Shift_JIS]C[日本語])";
        let bytes = encode_sgf(source_sgf, SourceEncoding::ShiftJis)
            .expect("fixture must be representable as Shift_JIS");

        let decoded = decode_sgf_bytes(&bytes).expect("declared Shift_JIS must decode");

        assert_eq!(decoded.content, source_sgf);
        assert_eq!(decoded.encoding, SourceEncoding::ShiftJis);
    }

    #[test]
    fn rejects_non_utf8_sgf_without_a_ca_declaration() {
        let error = decode_sgf_bytes(b"(;FF[4]C[\xff])")
            .expect_err("non-UTF-8 SGF without CA must be rejected");

        assert!(matches!(error, FileCodecError::InvalidUtf8WithoutCa));
    }

    #[test]
    fn rejects_unsupported_ca_encodings() {
        let error = decode_sgf_bytes(b"(;FF[4]CA[KOI8-R]C[\xff])")
            .expect_err("unsupported CA must not decode");

        assert!(matches!(error, FileCodecError::UnsupportedEncoding { .. }));
        assert!(error.to_string().contains("unsupported CA encoding"));
    }

    #[test]
    fn rejects_lossy_legacy_encoding() {
        let original_sgf = "(;FF[4]CA[Shift_JIS]C[日本語])";
        let edited_sgf = "(;FF[4]CA[Shift_JIS]C[日本語 😀])";

        assert!(matches!(
            encode_sgf(edited_sgf, SourceEncoding::ShiftJis),
            Err(FileCodecError::NotLosslesslyEncodable { .. })
        ));
        assert!(encode_sgf(original_sgf, SourceEncoding::ShiftJis).is_ok());
    }

    #[test]
    fn every_supported_encoding_round_trips_its_content() {
        for encoding in [
            SourceEncoding::Utf8,
            SourceEncoding::ShiftJis,
            SourceEncoding::EucJp,
            SourceEncoding::Gbk,
            SourceEncoding::Big5,
        ] {
            let source_sgf = match encoding {
                SourceEncoding::Utf8 => "(;FF[4]CA[UTF-8]C[日本語])".to_owned(),
                SourceEncoding::ShiftJis => "(;FF[4]CA[Shift_JIS]C[日本語])".to_owned(),
                SourceEncoding::EucJp => "(;FF[4]CA[EUC-JP]C[日本語])".to_owned(),
                SourceEncoding::Gbk => "(;FF[4]CA[GBK]C[中文])".to_owned(),
                SourceEncoding::Big5 => "(;FF[4]CA[Big5]C[中文])".to_owned(),
            };
            let bytes = encode_sgf(&source_sgf, encoding).expect("fixture must encode");
            let decoded = decode_sgf_bytes(&bytes).expect("declared encoding must decode");

            assert_eq!(decoded.content, source_sgf);
            assert_eq!(decoded.encoding, encoding);
        }
    }

    #[test]
    fn missing_ca_defaults_to_utf8() {
        let decoded = decode_sgf_bytes(b"(;FF[4]SZ[19])").expect("plain UTF-8 must decode");
        assert_eq!(decoded.encoding, SourceEncoding::Utf8);
        assert_eq!(detect_sgf_encoding(b"(;FF[4]SZ[19])").unwrap(), None);
    }

    #[test]
    fn strips_a_utf8_byte_order_mark() {
        let mut bytes = b"\xef\xbb\xbf".to_vec();
        bytes.extend_from_slice(b"(;FF[4]SZ[19])");

        let decoded = decode_sgf_bytes(&bytes).expect("BOM-prefixed UTF-8 must decode");

        assert_eq!(decoded.content, "(;FF[4]SZ[19])");
    }

    #[test]
    fn normalizes_encoding_label_spelling() {
        assert_eq!(
            detect_sgf_encoding(b"(;FF[4]CA[shift-jis])").unwrap(),
            Some(SourceEncoding::ShiftJis)
        );
        assert_eq!(
            detect_sgf_encoding(b"(;FF[4]CA[GB2312])").unwrap(),
            Some(SourceEncoding::Gbk)
        );
        assert_eq!(
            detect_sgf_encoding(b"(;FF[4]CA[utf_8])").unwrap(),
            Some(SourceEncoding::Utf8)
        );
    }

    #[test]
    fn malformed_ca_properties_are_reported() {
        assert!(matches!(
            detect_sgf_encoding(b"(;FF[4]CA[Shift_JIS)").unwrap_err(),
            FileCodecError::UnclosedCaProperty
        ));
        assert!(matches!(
            detect_sgf_encoding(b"(;FF[4]CA[]SZ[19])").unwrap_err(),
            FileCodecError::InvalidCaLabel
        ));
    }

    #[test]
    fn ca_inside_property_values_is_not_a_declaration() {
        assert_eq!(
            detect_sgf_encoding(b"(;FF[4]C[CA[Shift_JIS]])").unwrap(),
            None
        );
        assert_eq!(
            detect_sgf_encoding(b"(;FF[4]XCA[Shift_JIS])").unwrap(),
            None
        );
    }
}
