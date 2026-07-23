#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc,
    clippy::cast_possible_wrap
)]

use std::mem::size_of;
use zerocopy::FromBytes;

use crate::format::{BBFAssetEntry, BBFFooter, BBFHeader, BBFMetadata, BBFPageEntry, BBFSection};

#[derive(Debug, thiserror::Error)]
pub enum BBFError {
    #[error("Invalid BBF Magic")]
    InvalidMagic,
    #[error("File too short or corrupted header")]
    FileTooShort,
    #[error("Table error or invalid offsets")]
    TableError,
    #[error("Index out of bounds")]
    OutOfBounds,
}

pub struct BBFReader<T: AsRef<[u8]>> {
    data: T,
    pub header: BBFHeader,
    pub footer: BBFFooter,
}

impl<T: AsRef<[u8]>> BBFReader<T> {
    pub fn new(data: T) -> Result<Self, BBFError> {
        let slice = data.as_ref();
        let total_len = slice.len() as u64;

        if total_len < (size_of::<BBFHeader>() + size_of::<BBFFooter>()) as u64 {
            return Err(BBFError::FileTooShort);
        }

        let header_slice = &slice[..size_of::<BBFHeader>()];
        let header =
            BBFHeader::read_from_bytes(header_slice).map_err(|_| BBFError::FileTooShort)?;

        if &header.magic != b"BBF3" {
            return Err(BBFError::InvalidMagic);
        }

        let footer_offset = header.footer_offset.get() as usize;

        if footer_offset + size_of::<BBFFooter>() > slice.len() {
            return Err(BBFError::FileTooShort);
        }

        let footer_slice = &slice[footer_offset..footer_offset + size_of::<BBFFooter>()];
        let footer =
            BBFFooter::read_from_bytes(footer_slice).map_err(|_| BBFError::FileTooShort)?;

        let check_range = |offset: u64, count: u64, elem_size: usize| -> Result<(), BBFError> {
            let start = offset;
            let size = count
                .checked_mul(elem_size as u64)
                .ok_or(BBFError::TableError)?;
            let end = start.checked_add(size).ok_or(BBFError::TableError)?;

            if end > total_len {
                return Err(BBFError::FileTooShort);
            }
            Ok(())
        };

        check_range(
            footer.asset_offset.get(),
            footer.asset_count.get(),
            size_of::<BBFAssetEntry>(),
        )?;
        check_range(
            footer.page_offset.get(),
            footer.page_count.get(),
            size_of::<BBFPageEntry>(),
        )?;
        check_range(
            footer.section_offset.get(),
            footer.section_count.get(),
            size_of::<BBFSection>(),
        )?;
        check_range(
            footer.meta_offset.get(),
            footer.meta_count.get(),
            size_of::<BBFMetadata>(),
        )?;

        let pool_start = footer.string_pool_offset.get();
        let pool_size = footer.string_pool_size.get();
        if pool_start
            .checked_add(pool_size)
            .ok_or(BBFError::TableError)?
            > total_len
        {
            return Err(BBFError::FileTooShort);
        }

        Ok(Self {
            data,
            header,
            footer,
        })
    }

    fn get_table_slice<U: FromBytes + zerocopy::Immutable>(&self, offset: u64, count: u64) -> &[U] {
        let start = offset as usize;
        let elem_size = size_of::<U>();
        let len = (count as usize) * elem_size;

        let byte_slice = &self.data.as_ref()[start..start + len];

        <[U]>::ref_from_bytes(byte_slice).unwrap_or(&[])
    }

    pub fn assets(&self) -> &[BBFAssetEntry] {
        self.get_table_slice(
            self.footer.asset_offset.get(),
            self.footer.asset_count.get(),
        )
    }

    pub fn pages(&self) -> &[BBFPageEntry] {
        self.get_table_slice(self.footer.page_offset.get(), self.footer.page_count.get())
    }

    pub fn sections(&self) -> &[BBFSection] {
        self.get_table_slice(
            self.footer.section_offset.get(),
            self.footer.section_count.get(),
        )
    }

    pub fn metadata(&self) -> &[BBFMetadata] {
        self.get_table_slice(self.footer.meta_offset.get(), self.footer.meta_count.get())
    }

    pub fn get_string(&self, offset: u64) -> Option<&str> {
        let pool_start = self.footer.string_pool_offset.get() as usize;
        let pool_size = self.footer.string_pool_size.get() as usize;

        let offset = offset as usize;

        if offset < pool_start || offset >= pool_start + pool_size {
            return None;
        }

        let slice_from_offset = &self.data.as_ref()[offset..pool_start + pool_size];
        let end = slice_from_offset
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(slice_from_offset.len());

        std::str::from_utf8(&slice_from_offset[..end]).ok()
    }

    pub fn get_asset(&self, asset_index: u64) -> Result<&[u8], BBFError> {
        let assets = self.assets();
        if asset_index as usize >= assets.len() {
            return Err(BBFError::OutOfBounds);
        }

        let asset = &assets[asset_index as usize];
        let offset = asset.file_offset.get() as usize;
        let length = asset.file_size.get() as usize;

        let total_slice = self.data.as_ref();

        if offset.checked_add(length).ok_or(BBFError::OutOfBounds)? > total_slice.len() {
            return Err(BBFError::FileTooShort);
        }

        Ok(&total_slice[offset..offset + length])
    }
}
