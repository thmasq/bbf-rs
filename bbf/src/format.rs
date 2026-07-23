use zerocopy::byteorder::LittleEndian;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};
use zerocopy::{U16, U32, U64};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BBFMediaType {
    #[default]
    Unknown = 0x00,
    Avif = 0x01,
    Png = 0x02,
    Webp = 0x03,
    Jxl = 0x04,
    Bmp = 0x05,
    Gif = 0x07,
    Tiff = 0x08,
    Jpg = 0x09,
}

impl From<u8> for BBFMediaType {
    fn from(v: u8) -> Self {
        match v {
            0x01 => Self::Avif,
            0x02 => Self::Png,
            0x03 => Self::Webp,
            0x04 => Self::Jxl,
            0x05 => Self::Bmp,
            0x07 => Self::Gif,
            0x08 => Self::Tiff,
            0x09 => Self::Jpg,
            _ => Self::Unknown,
        }
    }
}

impl BBFMediaType {
    #[must_use]
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            ".png" => Self::Png,
            ".jpg" | ".jpeg" => Self::Jpg,
            ".avif" => Self::Avif,
            ".webp" => Self::Webp,
            ".jxl" => Self::Jxl,
            ".bmp" => Self::Bmp,
            ".gif" => Self::Gif,
            ".tiff" => Self::Tiff,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub const fn as_extension(&self) -> &'static str {
        match self {
            Self::Png => ".png",
            Self::Jpg => ".jpg",
            Self::Avif => ".avif",
            Self::Webp => ".webp",
            Self::Jxl => ".jxl",
            Self::Bmp => ".bmp",
            Self::Gif => ".gif",
            Self::Tiff => ".tiff",
            Self::Unknown => ".bin",
        }
    }
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFHeader {
    pub magic: [u8; 4], // "BBF3"
    pub version: U16<LittleEndian>,
    pub header_len: U16<LittleEndian>,
    pub flags: U32<LittleEndian>,
    pub alignment: u8,
    pub ream_size: u8,
    pub reserved_extra: U16<LittleEndian>,
    pub footer_offset: U64<LittleEndian>,
    pub reserved: [u8; 40],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFAssetEntry {
    pub file_offset: U64<LittleEndian>,
    pub asset_hash: [U64<LittleEndian>; 2],
    pub file_size: U64<LittleEndian>,
    pub flags: U32<LittleEndian>,
    pub reserved_value: U16<LittleEndian>,
    pub type_: u8,
    pub reserved: [u8; 9],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFPageEntry {
    pub asset_index: U64<LittleEndian>,
    pub flags: U32<LittleEndian>,
    pub reserved: [u8; 4],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFSection {
    pub section_title_offset: U64<LittleEndian>,
    pub section_start_index: U64<LittleEndian>,
    pub section_parent_offset: U64<LittleEndian>,
    pub reserved: [u8; 8],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFMetadata {
    pub key_offset: U64<LittleEndian>,
    pub value_offset: U64<LittleEndian>,
    pub parent_offset: U64<LittleEndian>,
    pub reserved: [u8; 8],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFExpansionEntry {
    pub exp_reserved: [U64<LittleEndian>; 10],
    pub flags: U32<LittleEndian>,
    pub reserved: [u8; 44],
}

#[repr(C, packed)]
#[derive(IntoBytes, FromBytes, Immutable, KnownLayout, Unaligned, Debug, Clone, Copy)]
pub struct BBFFooter {
    pub asset_offset: U64<LittleEndian>,
    pub page_offset: U64<LittleEndian>,
    pub section_offset: U64<LittleEndian>,
    pub meta_offset: U64<LittleEndian>,
    pub expansion_offset: U64<LittleEndian>,

    pub string_pool_offset: U64<LittleEndian>,
    pub string_pool_size: U64<LittleEndian>,

    pub asset_count: U64<LittleEndian>,
    pub page_count: U64<LittleEndian>,
    pub section_count: U64<LittleEndian>,
    pub meta_count: U64<LittleEndian>,
    pub expansion_count: U64<LittleEndian>,

    pub flags: U32<LittleEndian>,
    pub footer_len: U16<LittleEndian>,
    pub padding: [u8; 2],

    pub footer_hash: U64<LittleEndian>,

    pub reserved: [u8; 144],
}
