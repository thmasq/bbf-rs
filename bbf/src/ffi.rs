#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::CStr;
use std::fs::File;
use std::os::raw::c_char;
use std::panic;
use std::ptr;
use std::slice;

use crate::builder::BBFBuilder;
use crate::format::BBFMediaType;
use crate::reader::BBFReader;

pub struct CBbfBuilder(BBFBuilder<File>);

/// Creates a new BBF Builder that writes to the specified file path.
///
/// Returns a pointer to the builder object, or NULL if the file could not be created.
/// The caller owns the returned pointer and must eventually call `bbf_builder_finalize`
/// to free the memory and close the file.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_builder_new(path: *const c_char) -> *mut CBbfBuilder {
    let result = panic::catch_unwind(|| {
        if path.is_null() {
            return ptr::null_mut();
        }

        let c_str = unsafe { CStr::from_ptr(path) };
        let Ok(str_slice) = c_str.to_str() else {
            return ptr::null_mut();
        };

        let Ok(file) = File::create(str_slice) else {
            return ptr::null_mut();
        };

        BBFBuilder::new(file).map_or(ptr::null_mut(), |builder| {
            Box::into_raw(Box::new(CBbfBuilder(builder)))
        })
    });

    result.unwrap_or(ptr::null_mut())
}

/// Adds a page to the BBF file.
///
/// * `builder` - Pointer to the builder instance.
/// * `data` - Pointer to the raw image data.
/// * `len` - Length of the image data in bytes.
/// * `media_type` - The format of the image data (e.g., PNG, JPEG).
/// * `flags` - Optional flags for the page (usually 0).
///
/// Returns the asset index on success, or 0xFFFFFFFF ((uint32_t)-1) on failure.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_builder_add_page(
    builder: *mut CBbfBuilder,
    data: *const u8,
    len: usize,
    media_type: BBFMediaType,
    flags: u32,
) -> u32 {
    let result = panic::catch_unwind(|| {
        if builder.is_null() || (len > 0 && data.is_null()) {
            return 0xFFFF_FFFF;
        }

        let builder_ref = unsafe { &mut (*builder).0 };
        let slice = unsafe { slice::from_raw_parts(data, len) };

        builder_ref
            .add_page(slice, media_type, flags)
            .unwrap_or(0xFFFF_FFFF)
    });

    result.unwrap_or(0xFFFF_FFFF)
}

/// Finalizes the BBF file, writes the index, closes the file, and frees the builder memory.
///
/// This function consumes the builder pointer. You must not use the pointer after
/// calling this function.
///
/// Returns 0 on success, -1 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_builder_finalize(builder: *mut CBbfBuilder) -> i32 {
    let result = panic::catch_unwind(|| {
        if builder.is_null() {
            return -1;
        }
        let builder_box = unsafe { Box::from_raw(builder) };
        match builder_box.0.finalize() {
            Ok(()) => 0,
            Err(_) => -1,
        }
    });

    result.unwrap_or(-1)
}

pub struct CBbfReader(BBFReader<&'static [u8]>);

/// Creates a new reader from a memory buffer.
///
/// SAFETY: The `data` pointer must remain valid and unmodified until
/// `bbf_reader_free` is called. The reader does not copy the buffer;
/// it reads directly from the provided pointer.
///
/// Returns NULL if the data is not a valid BBF file or memory allocation fails.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_reader_new(data: *const u8, len: usize) -> *mut CBbfReader {
    let result = panic::catch_unwind(|| {
        if data.is_null() {
            return ptr::null_mut();
        }

        let slice = unsafe { slice::from_raw_parts(data, len) };

        let static_slice: &'static [u8] = unsafe { std::mem::transmute(slice) };

        BBFReader::new(static_slice).map_or(ptr::null_mut(), |reader| {
            Box::into_raw(Box::new(CBbfReader(reader)))
        })
    });

    result.unwrap_or(ptr::null_mut())
}

/// Frees the BBF Reader structure.
///
/// This does NOT free the buffer passed to `bbf_reader_new`. Managing the
/// backing buffer is the responsibility of the caller.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_reader_free(reader: *mut CBbfReader) {
    if !reader.is_null() {
        let _ = unsafe { Box::from_raw(reader) };
    }
}

/// Returns the number of pages in the BBF file.
/// Returns 0 if the reader pointer is NULL.
#[unsafe(no_mangle)]
pub extern "C" fn bbf_reader_get_page_count(reader: *mut CBbfReader) -> u32 {
    let result = panic::catch_unwind(|| {
        if reader.is_null() {
            return 0;
        }
        unsafe { (*reader).0.footer.page_count.get() as u32 }
    });

    result.unwrap_or(0)
}

/// Retrieves the data pointer and length for a specific page.
///
/// * `reader` - Pointer to the reader instance.
/// * `page_index` - Zero-based index of the page to retrieve.
/// * `out_ptr` - Output parameter that will receive the pointer to the image data.
/// * `out_len` - Output parameter that will receive the length of the data.
///
/// Returns 0 on success, -1 on failure (e.g., index out of bounds).
#[unsafe(no_mangle)]
pub extern "C" fn bbf_reader_get_page(
    reader: *mut CBbfReader,
    page_index: u32,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    let result = panic::catch_unwind(|| {
        if reader.is_null() || out_ptr.is_null() || out_len.is_null() {
            return -1;
        }

        let reader_ref = unsafe { &(*reader).0 };
        let pages = reader_ref.pages();

        if page_index as usize >= pages.len() {
            return -1;
        }

        let page = &pages[page_index as usize];
        let asset_index = page.asset_index.get();

        match reader_ref.get_asset(asset_index) {
            Ok(data_slice) => {
                unsafe {
                    *out_ptr = data_slice.as_ptr();
                    *out_len = data_slice.len();
                }
                0
            }
            Err(_) => -1,
        }
    });
    result.unwrap_or(-1)
}
