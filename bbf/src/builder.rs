#![allow(clippy::cast_possible_truncation, clippy::missing_errors_doc)]

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use xxhash_rust::xxh3::{Xxh3, xxh3_128};
use zerocopy::{FromBytes, FromZeros, IntoBytes};

use crate::format::{
    BBFAssetEntry, BBFFooter, BBFHeader, BBFMediaType, BBFMetadata, BBFPageEntry, BBFSection,
};

pub struct BBFBuilder<W: Write + Seek> {
    writer: W,
    current_offset: u64,

    assets: Vec<BBFAssetEntry>,
    pages: Vec<BBFPageEntry>,
    sections: Vec<BBFSection>,
    metadata: Vec<BBFMetadata>,
    string_pool: Vec<u8>,

    dedupe_map: HashMap<u128, u32>,
    string_map: HashMap<String, u64>,
}

impl<W: Write + Seek> BBFBuilder<W> {
    pub fn new(mut writer: W) -> io::Result<Self> {
        let header = BBFHeader {
            magic: *b"BBF3",
            version: 3.into(),
            header_len: (std::mem::size_of::<BBFHeader>() as u16).into(),
            flags: 0.into(),
            alignment: 12,
            ream_size: 16,
            reserved_extra: 0.into(),
            footer_offset: 0.into(), // This value will be patched later
            reserved: [0; 40],
        };

        writer.write_all(header.as_bytes())?;
        let current_offset = std::mem::size_of::<BBFHeader>() as u64;

        Ok(Self {
            writer,
            current_offset,
            assets: Vec::new(),
            pages: Vec::new(),
            sections: Vec::new(),
            metadata: Vec::new(),
            string_pool: Vec::new(),
            dedupe_map: HashMap::new(),
            string_map: HashMap::new(),
        })
    }

    fn align_padding(&mut self) -> io::Result<()> {
        let padding = (4096 - (self.current_offset % 4096)) % 4096;
        if padding > 0 {
            let zeroes = vec![0u8; padding as usize];
            self.writer.write_all(&zeroes)?;
            self.current_offset += padding;
        }
        Ok(())
    }

    pub fn add_page(
        &mut self,
        data: &[u8],
        media_type: BBFMediaType,
        flags: u32,
    ) -> io::Result<u32> {
        let hash = xxh3_128(data);
        let asset_index;

        if let Some(&idx) = self.dedupe_map.get(&hash) {
            asset_index = idx;
        } else {
            self.align_padding()?;

            let offset = self.current_offset;
            let length = data.len() as u64;

            self.writer.write_all(data)?;
            self.current_offset += length;

            let hash_lo = (hash & 0xFFFF_FFFF_FFFF_FFFF) as u64;
            let hash_hi = (hash >> 64) as u64;

            let entry = BBFAssetEntry {
                file_offset: offset.into(),
                asset_hash: [hash_lo.into(), hash_hi.into()],
                file_size: length.into(),
                flags: 0.into(),
                reserved_value: 0.into(),
                type_: media_type as u8,
                reserved: [0; 9],
            };

            asset_index = self.assets.len() as u32;
            self.assets.push(entry);
            self.dedupe_map.insert(hash, asset_index);
        }

        self.pages.push(BBFPageEntry {
            asset_index: u64::from(asset_index).into(),
            flags: flags.into(),
            reserved: [0; 4],
        });

        Ok(asset_index)
    }

    fn get_or_add_str(&mut self, s: &str) -> u64 {
        if let Some(&offset) = self.string_map.get(s) {
            return offset;
        }

        let offset = self.string_pool.len() as u64;
        self.string_pool.extend_from_slice(s.as_bytes());
        self.string_pool.push(0);
        self.string_map.insert(s.to_string(), offset);
        offset
    }

    pub fn add_section(&mut self, title: &str, start_page: u32, parent_idx: Option<u32>) {
        let section = BBFSection {
            section_title_offset: self.get_or_add_str(title).into(),
            section_start_index: u64::from(start_page).into(),
            section_parent_offset: parent_idx
                .map_or(0xFFFF_FFFF_FFFF_FFFF, |v| u64::from(v))
                .into(),
            reserved: [0; 8],
        };
        self.sections.push(section);
    }

    pub fn add_metadata(&mut self, key: &str, value: &str) {
        let meta = BBFMetadata {
            key_offset: self.get_or_add_str(key).into(),
            value_offset: self.get_or_add_str(value).into(),
            parent_offset: 0xFFFF_FFFF_FFFF_FFFF.into(),
            reserved: [0; 8],
        };
        self.metadata.push(meta);
    }

    pub fn finalize(self) -> io::Result<()> {
        let Self {
            mut writer,
            mut current_offset,
            assets,
            pages,
            mut sections,
            mut metadata,
            string_pool,
            ..
        } = self;

        let mut hasher = Xxh3::new();
        let mut footer = BBFFooter::new_zeroed();

        let string_pool_offset = current_offset
            + (assets.len() * std::mem::size_of::<BBFAssetEntry>()) as u64
            + (pages.len() * std::mem::size_of::<BBFPageEntry>()) as u64
            + (sections.len() * std::mem::size_of::<BBFSection>()) as u64
            + (metadata.len() * std::mem::size_of::<BBFMetadata>()) as u64;

        footer.string_pool_offset = string_pool_offset.into();
        footer.string_pool_size = (string_pool.len() as u64).into();

        for section in &mut sections {
            let relative = section.section_title_offset.get();
            section.section_title_offset = (relative + string_pool_offset).into();

            let parent = section.section_parent_offset.get();
            if parent != 0xFFFF_FFFF_FFFF_FFFF {
                section.section_parent_offset = (parent + string_pool_offset).into();
            }
        }
        for meta in &mut metadata {
            let relative_key = meta.key_offset.get();
            meta.key_offset = (relative_key + string_pool_offset).into();

            let relative_val = meta.value_offset.get();
            meta.value_offset = (relative_val + string_pool_offset).into();

            let parent = meta.parent_offset.get();
            if parent != 0xFFFF_FFFF_FFFF_FFFF {
                meta.parent_offset = (parent + string_pool_offset).into();
            }
        }

        macro_rules! write_hash {
            ($slice:expr) => {
                if !$slice.is_empty() {
                    writer.write_all($slice)?;
                    hasher.update($slice);
                    current_offset += $slice.len() as u64;
                }
            };
        }

        footer.asset_offset = current_offset.into();
        footer.asset_count = (assets.len() as u64).into();
        for asset in &assets {
            write_hash!(asset.as_bytes());
        }

        footer.page_offset = current_offset.into();
        footer.page_count = (pages.len() as u64).into();
        for page in &pages {
            write_hash!(page.as_bytes());
        }

        footer.section_offset = current_offset.into();
        footer.section_count = (sections.len() as u64).into();
        for section in &sections {
            write_hash!(section.as_bytes());
        }

        footer.meta_offset = current_offset.into();
        footer.meta_count = (metadata.len() as u64).into();
        for meta in &metadata {
            write_hash!(meta.as_bytes());
        }

        write_hash!(&string_pool);

        footer.expansion_offset = 0.into();
        footer.expansion_count = 0.into();

        footer.flags = 0.into();
        footer.footer_len = 256.into();
        footer.padding = [0; 2];

        footer.footer_hash = hasher.digest().into();

        let footer_offset = current_offset;
        writer.write_all(footer.as_bytes())?;

        writer.seek(std::io::SeekFrom::Start(16))?;
        writer.write_all(&footer_offset.to_le_bytes())?;

        Ok(())
    }
}

/// Post-processes a standard BBF file into a "petrified" BBF file in-place.
pub fn petrify<F: Read + Write + Seek>(file: &mut F) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut header_buf = [0u8; std::mem::size_of::<BBFHeader>()];
    file.read_exact(&mut header_buf)?;

    let mut header = BBFHeader::read_from_bytes(&header_buf)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid header"))?;

    let old_footer_offset = header.footer_offset.get() as u64;

    file.seek(SeekFrom::Start(old_footer_offset))?;
    let mut footer_buf = [0u8; std::mem::size_of::<BBFFooter>()];
    file.read_exact(&mut footer_buf)?;

    let old_footer = BBFFooter::read_from_bytes(&footer_buf)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid footer"))?;

    let asset_count = old_footer.asset_count.get() as usize;
    let page_count = old_footer.page_count.get() as usize;
    let section_count = old_footer.section_count.get() as usize;
    let meta_count = old_footer.meta_count.get() as usize;
    let pool_size = old_footer.string_pool_size.get() as usize;

    let tables_size = (asset_count * std::mem::size_of::<BBFAssetEntry>())
        + (page_count * std::mem::size_of::<BBFPageEntry>())
        + (section_count * std::mem::size_of::<BBFSection>())
        + (meta_count * std::mem::size_of::<BBFMetadata>())
        + pool_size;

    let shift_amount = (std::mem::size_of::<BBFFooter>() + tables_size) as u64;

    let mut tables_buf = vec![0u8; tables_size];
    file.seek(SeekFrom::Start(old_footer.asset_offset.get()))?;
    file.read_exact(&mut tables_buf)?;

    let (assets_bytes, rest) =
        tables_buf.split_at_mut(asset_count * std::mem::size_of::<BBFAssetEntry>());
    let (_pages_bytes, rest) = rest.split_at_mut(page_count * std::mem::size_of::<BBFPageEntry>());
    let (sections_bytes, rest) =
        rest.split_at_mut(section_count * std::mem::size_of::<BBFSection>());
    let (meta_bytes, _string_pool) =
        rest.split_at_mut(meta_count * std::mem::size_of::<BBFMetadata>());

    let assets = <[BBFAssetEntry]>::mut_from_bytes(assets_bytes).unwrap();
    let sections = <[BBFSection]>::mut_from_bytes(sections_bytes).unwrap();
    let metadata = <[BBFMetadata]>::mut_from_bytes(meta_bytes).unwrap();

    let mut new_footer = old_footer;
    let mut current_offset =
        (std::mem::size_of::<BBFHeader>() + std::mem::size_of::<BBFFooter>()) as u64;

    new_footer.asset_offset = current_offset.into();
    current_offset += (asset_count * std::mem::size_of::<BBFAssetEntry>()) as u64;

    new_footer.page_offset = current_offset.into();
    current_offset += (page_count * std::mem::size_of::<BBFPageEntry>()) as u64;

    new_footer.section_offset = current_offset.into();
    current_offset += (section_count * std::mem::size_of::<BBFSection>()) as u64;

    new_footer.meta_offset = current_offset.into();
    current_offset += (meta_count * std::mem::size_of::<BBFMetadata>()) as u64;

    new_footer.string_pool_offset = current_offset.into();

    let string_offset_diff =
        new_footer.string_pool_offset.get() as i64 - old_footer.string_pool_offset.get() as i64;

    for asset in assets.iter_mut() {
        let old_offset = asset.file_offset.get();
        asset.file_offset = (old_offset + shift_amount).into();
    }

    for section in sections.iter_mut() {
        section.section_title_offset =
            ((section.section_title_offset.get() as i64 + string_offset_diff) as u64).into();
        let parent = section.section_parent_offset.get();
        if parent != 0xFFFF_FFFF_FFFF_FFFF {
            section.section_parent_offset = ((parent as i64 + string_offset_diff) as u64).into();
        }
    }

    for meta in metadata.iter_mut() {
        meta.key_offset = ((meta.key_offset.get() as i64 + string_offset_diff) as u64).into();
        meta.value_offset = ((meta.value_offset.get() as i64 + string_offset_diff) as u64).into();
        let parent = meta.parent_offset.get();
        if parent != 0xFFFF_FFFF_FFFF_FFFF {
            meta.parent_offset = ((parent as i64 + string_offset_diff) as u64).into();
        }
    }

    let mut hasher = Xxh3::new();
    hasher.update(&tables_buf);
    new_footer.footer_hash = hasher.digest().into();

    let asset_data_start = std::mem::size_of::<BBFHeader>() as u64;
    let asset_data_end = old_footer_offset;

    let mut shift_buf = vec![0u8; 65536];
    let mut current_end = asset_data_end;

    while current_end > asset_data_start {
        let chunk_size = std::cmp::min(shift_buf.len() as u64, current_end - asset_data_start);
        let current_start = current_end - chunk_size;

        file.seek(SeekFrom::Start(current_start))?;
        file.read_exact(&mut shift_buf[..chunk_size as usize])?;

        file.seek(SeekFrom::Start(current_start + shift_amount))?;
        file.write_all(&shift_buf[..chunk_size as usize])?;

        current_end = current_start;
    }

    header.footer_offset = (std::mem::size_of::<BBFHeader>() as u64).into();

    file.seek(SeekFrom::Start(0))?;
    file.write_all(header.as_bytes())?;
    file.write_all(new_footer.as_bytes())?;
    file.write_all(&tables_buf)?;

    Ok(())
}
