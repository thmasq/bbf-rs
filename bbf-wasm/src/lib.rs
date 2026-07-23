use bbf::BBFMediaType;
use bbf::BBFReader;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmBBFStreamer {
    buffer: Vec<u8>,
}

#[wasm_bindgen]
impl WasmBBFStreamer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Appends a newly downloaded chunk from JS to the internal Wasm buffer
    pub fn append_chunk(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// Checks if the index is fully downloaded and parsed (instantly true for Petrified files)
    pub fn is_ready(&self) -> bool {
        BBFReader::new(&self.buffer).is_ok()
    }

    /// Gets total pages (Call this only after is_ready() returns true)
    #[wasm_bindgen(getter)]
    pub fn page_count(&self) -> Result<u32, JsValue> {
        let reader =
            BBFReader::new(&self.buffer).map_err(|_| JsValue::from_str("Index not ready yet"))?;
        Ok(reader.pages().len() as u32)
    }

    /// Checks if a specific page's bytes have fully arrived
    pub fn is_page_ready(&self, page_index: u32) -> Result<bool, JsValue> {
        let reader =
            BBFReader::new(&self.buffer).map_err(|_| JsValue::from_str("Index not ready yet"))?;

        let pages = reader.pages();
        if page_index as usize >= pages.len() {
            return Err(JsValue::from_str("Page index out of bounds"));
        }

        let asset_idx = pages[page_index as usize].asset_index.get();
        Ok(reader.get_asset(asset_idx).is_ok())
    }

    /// Extracts the page data (Call this only after is_page_ready() returns true)
    pub fn get_page(&self, page_index: u32) -> Result<Uint8Array, JsValue> {
        let reader =
            BBFReader::new(&self.buffer).map_err(|_| JsValue::from_str("Index not ready yet"))?;

        let pages = reader.pages();
        if page_index as usize >= pages.len() {
            return Err(JsValue::from_str("Page index out of bounds"));
        }

        let asset_idx = pages[page_index as usize].asset_index.get();
        let data = reader
            .get_asset(asset_idx)
            .map_err(|_| JsValue::from_str("Page data has not fully downloaded yet"))?;

        Ok(Uint8Array::from(data))
    }

    /// Gets the MIME type of a specific page
    pub fn get_page_mime(&self, page_index: u32) -> Result<String, JsValue> {
        let reader =
            BBFReader::new(&self.buffer).map_err(|_| JsValue::from_str("Index not ready yet"))?;

        let pages = reader.pages();
        if page_index as usize >= pages.len() {
            return Err(JsValue::from_str("Page index out of bounds"));
        }

        let asset_idx = pages[page_index as usize].asset_index.get();
        let assets = reader.assets();
        let asset = &assets[asset_idx as usize];

        let mime = BBFMediaType::from(asset.type_).as_extension();
        let mime_str = match mime {
            ".png" => "image/png",
            ".jpg" | ".jpeg" => "image/jpeg",
            ".avif" => "image/avif",
            ".webp" => "image/webp",
            _ => "application/octet-stream",
        };

        Ok(mime_str.to_string())
    }
}
