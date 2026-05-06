//! # Tuple Representation
//!
//! A tuple is the fundamental unit of data in AtlasDB. Each tuple represents
//! a single row in a table.
//!
//! ## Tuple Layout (on disk / in page)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  Tuple Header (12 bytes)             │
//! │  ┌──────────┬──────────┬──────────┬────────────────┐│
//! │  │  txn_id  │  flags   │ null_bmp │   field_count  ││
//! │  │ (4 bytes)│ (2 bytes)│ (2 bytes)│   (2 bytes)    ││
//! │  └──────────┴──────────┴──────────┴────────────────┘│
//! │  │  reserved (2 bytes)                              │
//! ├─────────────────────────────────────────────────────┤
//! │                  Null Bitmap                         │
//! │  (ceil(field_count / 8) bytes, follows header)      │
//! ├─────────────────────────────────────────────────────┤
//! │                  Field Data                          │
//! │  Fixed-length fields stored inline                   │
//! │  Variable-length: 4-byte offset+length, data at end │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Design Decisions
//!
//! **Transaction ID in header**: Every tuple carries its creating transaction's
//! ID. This is essential for MVCC (Phase 9) — PostgreSQL stores `xmin`/`xmax`
//! similarly.
//!
//! **Null bitmap**: Rather than using sentinel values (wastes space for large
//! types) or optional wrappers (overhead), we use a compact bitmap. This is
//! identical to PostgreSQL's approach.
//!
//! **Variable-length fields**: We store a 4-byte descriptor (2 bytes offset,
//! 2 bytes length) in the fixed portion, with actual data appended at the end
//! of the tuple. This avoids the complexity of PostgreSQL's varlena headers
//! while remaining efficient.

use std::fmt;

use super::error::{StorageError, StorageResult};

/// Size of the fixed tuple header in bytes.
pub const TUPLE_HEADER_SIZE: usize = 12;

/// Maximum number of fields per tuple.
pub const MAX_FIELDS: usize = 1024;

/// Tuple flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TupleFlags {
    /// Normal, live tuple.
    Live = 0x0001,
    /// Tuple has been deleted (but not yet vacuumed).
    Deleted = 0x0002,
    /// Tuple is being updated (in-progress transaction).
    Updating = 0x0004,
    /// Tuple has been committed.
    Committed = 0x0008,
    /// Tuple has been aborted (rollback).
    Aborted = 0x0010,
}

/// Header prepended to every tuple.
///
/// In PostgreSQL, this is called the "heap tuple header" and contains
/// `t_xmin`, `t_xmax`, `t_cid`, `t_ctid`, and `t_infomask`.
/// We simplify to the essentials needed for Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TupleHeader {
    /// Transaction ID that created this tuple (analogous to PostgreSQL's xmin).
    pub txn_id: u32,
    /// Status flags for this tuple.
    pub flags: u16,
    /// Number of fields in this tuple.
    pub field_count: u16,
    /// Size of the null bitmap in bytes.
    pub null_bitmap_size: u16,
    /// Reserved for future use (alignment padding).
    pub _reserved: u16,
}

impl TupleHeader {
    /// Create a new tuple header.
    pub fn new(txn_id: u32, field_count: u16) -> Self {
        let null_bitmap_size = (field_count as usize + 7) / 8;
        Self {
            txn_id,
            flags: TupleFlags::Live as u16,
            field_count,
            null_bitmap_size: null_bitmap_size as u16,
            _reserved: 0,
        }
    }

    /// Serialize the header into a byte buffer.
    pub fn serialize(&self, buf: &mut [u8]) -> StorageResult<()> {
        if buf.len() < TUPLE_HEADER_SIZE {
            return Err(StorageError::SerializationError(
                format!("buffer too small for tuple header: {} < {}", buf.len(), TUPLE_HEADER_SIZE)
            ));
        }
        buf[0..4].copy_from_slice(&self.txn_id.to_le_bytes());
        buf[4..6].copy_from_slice(&self.flags.to_le_bytes());
        buf[6..8].copy_from_slice(&self.field_count.to_le_bytes());
        buf[8..10].copy_from_slice(&self.null_bitmap_size.to_le_bytes());
        buf[10..12].copy_from_slice(&self._reserved.to_le_bytes());
        Ok(())
    }

    /// Deserialize a header from a byte buffer.
    pub fn deserialize(buf: &[u8]) -> StorageResult<Self> {
        if buf.len() < TUPLE_HEADER_SIZE {
            return Err(StorageError::SerializationError(
                format!("buffer too small for tuple header: {} < {}", buf.len(), TUPLE_HEADER_SIZE)
            ));
        }
        Ok(Self {
            txn_id: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            flags: u16::from_le_bytes(buf[4..6].try_into().unwrap()),
            field_count: u16::from_le_bytes(buf[6..8].try_into().unwrap()),
            null_bitmap_size: u16::from_le_bytes(buf[8..10].try_into().unwrap()),
            _reserved: u16::from_le_bytes(buf[10..12].try_into().unwrap()),
        })
    }

    /// Check if this tuple is live (not deleted).
    pub fn is_live(&self) -> bool {
        (self.flags & TupleFlags::Deleted as u16) == 0
    }

    /// Mark this tuple as deleted.
    pub fn mark_deleted(&mut self) {
        self.flags |= TupleFlags::Deleted as u16;
        self.flags &= !(TupleFlags::Live as u16);
    }
}

/// A field value in a tuple.
///
/// AtlasDB supports a core set of data types, extensible in future phases.
/// This enum is the in-memory representation; on-disk format is handled
/// during serialization.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// NULL value (represented in the null bitmap, not stored).
    Null,
    /// Boolean (1 byte on disk).
    Boolean(bool),
    /// 32-bit signed integer.
    Int32(i32),
    /// 64-bit signed integer.
    Int64(i64),
    /// 32-bit floating point.
    Float32(f32),
    /// 64-bit floating point.
    Float64(f64),
    /// Variable-length string (UTF-8).
    String(String),
    /// Variable-length binary data.
    Bytes(Vec<u8>),
}

impl FieldValue {
    /// Returns the serialized size of this field value in bytes.
    /// For variable-length types, this includes the 4-byte length prefix.
    pub fn serialized_size(&self) -> usize {
        match self {
            FieldValue::Null => 0,
            FieldValue::Boolean(_) => 1,
            FieldValue::Int32(_) => 4,
            FieldValue::Int64(_) => 8,
            FieldValue::Float32(_) => 4,
            FieldValue::Float64(_) => 8,
            // Variable-length: 4-byte length prefix + data
            FieldValue::String(s) => 4 + s.len(),
            FieldValue::Bytes(b) => 4 + b.len(),
        }
    }

    /// Returns a type tag byte for serialization.
    pub fn type_tag(&self) -> u8 {
        match self {
            FieldValue::Null => 0,
            FieldValue::Boolean(_) => 1,
            FieldValue::Int32(_) => 2,
            FieldValue::Int64(_) => 3,
            FieldValue::Float32(_) => 4,
            FieldValue::Float64(_) => 5,
            FieldValue::String(_) => 6,
            FieldValue::Bytes(_) => 7,
        }
    }

    /// Serialize this field value into a byte buffer.
    /// Returns the number of bytes written.
    pub fn serialize(&self, buf: &mut [u8]) -> StorageResult<usize> {
        match self {
            FieldValue::Null => Ok(0),
            FieldValue::Boolean(v) => {
                if buf.is_empty() {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0] = if *v { 1 } else { 0 };
                Ok(1)
            }
            FieldValue::Int32(v) => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..4].copy_from_slice(&v.to_le_bytes());
                Ok(4)
            }
            FieldValue::Int64(v) => {
                if buf.len() < 8 {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..8].copy_from_slice(&v.to_le_bytes());
                Ok(8)
            }
            FieldValue::Float32(v) => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..4].copy_from_slice(&v.to_le_bytes());
                Ok(4)
            }
            FieldValue::Float64(v) => {
                if buf.len() < 8 {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..8].copy_from_slice(&v.to_le_bytes());
                Ok(8)
            }
            FieldValue::String(s) => {
                let data = s.as_bytes();
                let total = 4 + data.len();
                if buf.len() < total {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
                buf[4..total].copy_from_slice(data);
                Ok(total)
            }
            FieldValue::Bytes(b) => {
                let total = 4 + b.len();
                if buf.len() < total {
                    return Err(StorageError::SerializationError("buffer too small".into()));
                }
                buf[0..4].copy_from_slice(&(b.len() as u32).to_le_bytes());
                buf[4..total].copy_from_slice(b);
                Ok(total)
            }
        }
    }

    /// Deserialize a field value from a byte buffer given its type tag.
    /// Returns the value and the number of bytes consumed.
    pub fn deserialize(tag: u8, buf: &[u8]) -> StorageResult<(Self, usize)> {
        match tag {
            0 => Ok((FieldValue::Null, 0)),
            1 => {
                if buf.is_empty() {
                    return Err(StorageError::SerializationError("buffer too small for bool".into()));
                }
                Ok((FieldValue::Boolean(buf[0] != 0), 1))
            }
            2 => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small for i32".into()));
                }
                let v = i32::from_le_bytes(buf[0..4].try_into().unwrap());
                Ok((FieldValue::Int32(v), 4))
            }
            3 => {
                if buf.len() < 8 {
                    return Err(StorageError::SerializationError("buffer too small for i64".into()));
                }
                let v = i64::from_le_bytes(buf[0..8].try_into().unwrap());
                Ok((FieldValue::Int64(v), 8))
            }
            4 => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small for f32".into()));
                }
                let v = f32::from_le_bytes(buf[0..4].try_into().unwrap());
                Ok((FieldValue::Float32(v), 4))
            }
            5 => {
                if buf.len() < 8 {
                    return Err(StorageError::SerializationError("buffer too small for f64".into()));
                }
                let v = f64::from_le_bytes(buf[0..8].try_into().unwrap());
                Ok((FieldValue::Float64(v), 8))
            }
            6 => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small for string length".into()));
                }
                let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
                if buf.len() < 4 + len {
                    return Err(StorageError::SerializationError("buffer too small for string data".into()));
                }
                let s = String::from_utf8(buf[4..4 + len].to_vec())
                    .map_err(|e| StorageError::SerializationError(format!("invalid UTF-8: {}", e)))?;
                Ok((FieldValue::String(s), 4 + len))
            }
            7 => {
                if buf.len() < 4 {
                    return Err(StorageError::SerializationError("buffer too small for bytes length".into()));
                }
                let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
                if buf.len() < 4 + len {
                    return Err(StorageError::SerializationError("buffer too small for bytes data".into()));
                }
                let b = buf[4..4 + len].to_vec();
                Ok((FieldValue::Bytes(b), 4 + len))
            }
            _ => Err(StorageError::SerializationError(format!("unknown type tag: {}", tag))),
        }
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::Null => write!(f, "NULL"),
            FieldValue::Boolean(v) => write!(f, "{}", v),
            FieldValue::Int32(v) => write!(f, "{}", v),
            FieldValue::Int64(v) => write!(f, "{}", v),
            FieldValue::Float32(v) => write!(f, "{:.6}", v),
            FieldValue::Float64(v) => write!(f, "{:.6}", v),
            FieldValue::String(v) => write!(f, "'{}'", v),
            FieldValue::Bytes(v) => write!(f, "0x{}", hex_encode(v)),
        }
    }
}

/// Simple hex encoding for display purposes.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// A complete tuple (row) with header and field values.
///
/// This is the in-memory representation. On disk, tuples are serialized
/// into the page's data region as raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuple {
    /// Tuple metadata header.
    pub header: TupleHeader,
    /// Type tags for each field (used for deserialization).
    pub type_tags: Vec<u8>,
    /// The field values in this tuple.
    pub fields: Vec<FieldValue>,
}

impl Tuple {
    /// Create a new tuple with the given transaction ID and fields.
    pub fn new(txn_id: u32, fields: Vec<FieldValue>) -> Self {
        let field_count = fields.len() as u16;
        let type_tags: Vec<u8> = fields.iter().map(|f| f.type_tag()).collect();
        Self {
            header: TupleHeader::new(txn_id, field_count),
            type_tags,
            fields,
        }
    }

    /// Calculate the total serialized size of this tuple.
    ///
    /// Layout: [header (12B)] [type_tags (N bytes)] [null_bitmap] [field data...]
    pub fn serialized_size(&self) -> usize {
        let header_size = TUPLE_HEADER_SIZE;
        let type_tags_size = self.type_tags.len();
        let null_bitmap_size = self.header.null_bitmap_size as usize;
        let field_data_size: usize = self.fields.iter().map(|f| f.serialized_size()).sum();
        header_size + type_tags_size + null_bitmap_size + field_data_size
    }

    /// Serialize this tuple into a byte buffer.
    /// Returns the number of bytes written.
    pub fn serialize(&self, buf: &mut [u8]) -> StorageResult<usize> {
        let total_size = self.serialized_size();
        if buf.len() < total_size {
            return Err(StorageError::SerializationError(
                format!("buffer too small: {} < {}", buf.len(), total_size)
            ));
        }

        let mut offset = 0;

        // 1. Write header
        self.header.serialize(&mut buf[offset..])?;
        offset += TUPLE_HEADER_SIZE;

        // 2. Write type tags
        buf[offset..offset + self.type_tags.len()].copy_from_slice(&self.type_tags);
        offset += self.type_tags.len();

        // 3. Write null bitmap
        let null_bitmap_size = self.header.null_bitmap_size as usize;
        // Zero out the bitmap first
        for b in &mut buf[offset..offset + null_bitmap_size] {
            *b = 0;
        }
        for (i, field) in self.fields.iter().enumerate() {
            if matches!(field, FieldValue::Null) {
                let byte_idx = i / 8;
                let bit_idx = i % 8;
                buf[offset + byte_idx] |= 1 << bit_idx;
            }
        }
        offset += null_bitmap_size;

        // 4. Write field data
        for field in &self.fields {
            let written = field.serialize(&mut buf[offset..])?;
            offset += written;
        }

        Ok(total_size)
    }

    /// Deserialize a tuple from a byte buffer.
    pub fn deserialize(buf: &[u8]) -> StorageResult<Self> {
        if buf.len() < TUPLE_HEADER_SIZE {
            return Err(StorageError::SerializationError(
                "buffer too small for tuple".into()
            ));
        }

        let mut offset = 0;

        // 1. Read header
        let header = TupleHeader::deserialize(&buf[offset..])?;
        offset += TUPLE_HEADER_SIZE;

        let field_count = header.field_count as usize;

        // 2. Read type tags
        if buf.len() < offset + field_count {
            return Err(StorageError::SerializationError(
                "buffer too small for type tags".into()
            ));
        }
        let type_tags: Vec<u8> = buf[offset..offset + field_count].to_vec();
        offset += field_count;

        // 3. Read null bitmap
        let null_bitmap_size = header.null_bitmap_size as usize;
        if buf.len() < offset + null_bitmap_size {
            return Err(StorageError::SerializationError(
                "buffer too small for null bitmap".into()
            ));
        }
        let null_bitmap = &buf[offset..offset + null_bitmap_size];
        offset += null_bitmap_size;

        // 4. Read field data
        let mut fields = Vec::with_capacity(field_count);
        for i in 0..field_count {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            let is_null = (null_bitmap[byte_idx] >> bit_idx) & 1 == 1;

            if is_null {
                fields.push(FieldValue::Null);
            } else {
                let (value, consumed) = FieldValue::deserialize(type_tags[i], &buf[offset..])?;
                fields.push(value);
                offset += consumed;
            }
        }

        Ok(Self {
            header,
            type_tags,
            fields,
        })
    }
}

impl fmt::Display for Tuple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        for (i, field) in self.fields.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", field)?;
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tuple_header_roundtrip() {
        let header = TupleHeader::new(42, 5);
        let mut buf = [0u8; TUPLE_HEADER_SIZE];
        header.serialize(&mut buf).unwrap();
        let restored = TupleHeader::deserialize(&buf).unwrap();
        assert_eq!(header, restored);
    }

    #[test]
    fn test_field_value_serialization() {
        let test_cases: Vec<FieldValue> = vec![
            FieldValue::Boolean(true),
            FieldValue::Int32(-42),
            FieldValue::Int64(1_000_000_000_000),
            FieldValue::Float32(3.14),
            FieldValue::Float64(2.718281828459045),
            FieldValue::String("hello world".into()),
            FieldValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        ];

        for value in &test_cases {
            let size = value.serialized_size();
            let mut buf = vec![0u8; size];
            let written = value.serialize(&mut buf).unwrap();
            assert_eq!(written, size, "size mismatch for {:?}", value);

            let (restored, consumed) = FieldValue::deserialize(value.type_tag(), &buf).unwrap();
            assert_eq!(consumed, size, "consumed mismatch for {:?}", value);
            assert_eq!(&restored, value, "value mismatch for {:?}", value);
        }
    }

    #[test]
    fn test_tuple_roundtrip() {
        let tuple = Tuple::new(1, vec![
            FieldValue::Int32(42),
            FieldValue::String("AtlasDB".into()),
            FieldValue::Null,
            FieldValue::Boolean(true),
            FieldValue::Float64(3.14159),
        ]);

        let size = tuple.serialized_size();
        let mut buf = vec![0u8; size];
        let written = tuple.serialize(&mut buf).unwrap();
        assert_eq!(written, size);

        let restored = Tuple::deserialize(&buf).unwrap();
        assert_eq!(tuple.header, restored.header);
        assert_eq!(tuple.fields, restored.fields);
    }

    #[test]
    fn test_tuple_with_nulls() {
        let tuple = Tuple::new(1, vec![
            FieldValue::Null,
            FieldValue::Null,
            FieldValue::Int32(100),
            FieldValue::Null,
        ]);

        let size = tuple.serialized_size();
        let mut buf = vec![0u8; size];
        tuple.serialize(&mut buf).unwrap();
        let restored = Tuple::deserialize(&buf).unwrap();
        assert_eq!(tuple.fields, restored.fields);
    }

    #[test]
    fn test_tuple_display() {
        let tuple = Tuple::new(1, vec![
            FieldValue::Int32(1),
            FieldValue::String("hello".into()),
            FieldValue::Null,
        ]);
        let display = format!("{}", tuple);
        assert_eq!(display, "(1, 'hello', NULL)");
    }
}
