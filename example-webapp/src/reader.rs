#![allow(clippy::cast_possible_truncation)]

use crate::utils::read_file_to_vec;
use bbf::{BBFMediaType, BBFReader};
use leptos::ev::{mousemove, mouseup};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_styling::inline_style_sheet;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, MouseEvent, Url, js_sys};

#[derive(Clone)]
struct LoadedBook {
    #[allow(dead_code)]
    name: String,
    reader: Arc<BBFReader<Arc<[u8]>>>,
}

#[allow(clippy::too_many_lines)]
#[component]
pub fn Reader() -> impl IntoView {
    let (book, set_book) = signal(Option::<LoadedBook>::None);
    let (page_idx, set_page_idx) = signal(0u32);
    let (img_url, set_img_url) = signal(String::new());
    let (status, set_status) = signal(String::new());

    let (sidebar_width, set_sidebar_width) = signal(250);
    let (is_resizing, set_is_resizing) = signal(false);

    inline_style_sheet! {
        reader_css,
        "reader",

        .container {
            height: 100%;
            display: flex;
            flex-direction: column;
            color: #e2e8f0; /* text-slate-200 */
        }

        .main-content {
            flex: 1;
            display: flex;
            overflow: hidden;
        }

        .empty-state {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            height: 100%;
            color: #64748b; /* text-slate-500 */
            cursor: pointer;
            transition: all 0.2s;
        }
        .empty-state:hover {
            color: #94a3b8; /* text-slate-400 */
            background-color: rgba(30, 41, 59, 0.5); /* bg-slate-800/50 */
        }
        .empty-icon { font-size: 3.75rem; margin-bottom: 1rem; opacity: 0.5; }
        .empty-text { font-size: 1.125rem; font-weight: 500; }

        .sidebar {
            background-color: #0f172a; /* bg-slate-900 */
            display: flex;
            flex-direction: column;
            overflow-y: auto;
            flex-shrink: 0;
            display: none;
        }

        /* Resizer Handle */
        .resizer {
            width: 5px;
            background-color: #1e293b; /* slate-800 */
            cursor: col-resize;
            flex-shrink: 0;
            transition: background-color 0.2s;
            display: none;
            z-index: 10;
        }
        .resizer:hover {
            background-color: #6366f1; /* indigo-500 */
        }
        .resizer-active {
            background-color: #6366f1; /* indigo-500 */
        }

        @media (min-width: 768px) {
            .sidebar { display: block; }
            .resizer { display: block; }
        }

        .sidebar-controls {
            padding: 1rem;
            background-color: #1e293b;
            border-bottom: 1px solid #334155;
            position: sticky;
            top: 0;
            z-index: 5;
        }

        .sidebar-btn {
            display: block;
            text-align: center;
            background-color: #4f46e5; /* bg-indigo-600 */
            color: white;
            padding: 0.5rem;
            border-radius: 0.5rem;
            cursor: pointer;
            font-size: 0.875rem;
            font-weight: 500;
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
            transition: background-color 0.2s;
            margin-bottom: 0.75rem;
        }
        .sidebar-btn:hover { background-color: #6366f1; }

        .status {
            color: #a5b4fc; /* text-indigo-300 */
            font-family: monospace;
            font-size: 0.75rem;
            text-align: center;
            word-break: break-word;
        }

        .sidebar-header {
            padding: 1rem;
            background-color: #1e293b; /* bg-slate-800 */
            border-bottom: 1px solid #334155;
            font-weight: 700;
            color: #e2e8f0;
        }

        .sidebar-header-meta {
            margin-top: 1rem;
            border-top: 1px solid #334155;
            border-bottom: 1px solid #334155;
        }

        .sidebar-list { padding: 0.5rem; list-style: none; margin: 0; }
        .meta-list { padding: 1rem; list-style: none; margin: 0; font-size: 0.75rem; color: #94a3b8; }

        .section-item {
            cursor: pointer;
            padding: 0.375rem 0.5rem;
            border-radius: 0.25rem;
            transition: background-color 0.2s;
            color: #94a3b8; /* Default text-slate-400 */
        }
        .section-item:hover { background-color: #1e293b; }

        .active {
            color: #818cf8; /* text-indigo-400 */
            background-color: #1e293b;
        }

        .section-title { font-weight: 500; font-size: 0.875rem; }
        .section-page { font-size: 0.75rem; opacity: 0.5; }

        .meta-item {
            display: flex;
            flex-direction: column;
            border-bottom: 1px solid #1e293b;
            padding-bottom: 0.25rem;
            margin-bottom: 0.5rem;
        }
        .meta-item:last-child { border-bottom: none; }
        .meta-key { color: #818cf8; font-weight: 700; }
        .meta-val { color: #cbd5e1; word-break: break-word; }

        .viewer-area {
            flex: 1;
            display: flex;
            flex-direction: column;
            background-color: black;
            position: relative;
            overflow: hidden;
        }

        .image-container {
            flex: 1;
            display: flex;
            align-items: center;
            justify-content: center;
            overflow: auto;
            padding: 0.5rem;
            cursor: pointer;
        }

        .page-image {
            max-height: 100%;
            max-width: 100%;
            object-fit: contain;
            box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.25);
        }

        .controls {
            background-color: #0f172a;
            border-top: 1px solid #334155;
            color: #e2e8f0;
            padding: 0.25rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
            z-index: 20;
        }

        .nav-btn {
            padding: 0.25rem 0.60rem;
            background-color: #1e293b;
            border: 1px solid #475569;
            border-radius: 0.25rem;
            font-size: 0.75rem;
            color: inherit;
            cursor: pointer;
            transition: background-color 0.2s;
        }
        .nav-btn:hover { background-color: #334155; }

        .page-counter { font-family: monospace; font-size: 0.875rem; color: #a5b4fc; }
        .page-number { color: white; font-weight: 700; }
    }

    let start_resize = move |ev: MouseEvent| {
        ev.prevent_default();
        set_is_resizing.set(true);
    };

    let handle = window_event_listener(mousemove, move |ev: MouseEvent| {
        if is_resizing.get() {
            ev.prevent_default();
            let new_width = ev.client_x();
            let clamped = new_width.clamp(150, 600);
            set_sidebar_width.set(clamped);
        }
    });

    on_cleanup(move || handle.remove());

    let handle_up = window_event_listener(mouseup, move |_| {
        set_is_resizing.set(false);
    });

    on_cleanup(move || handle_up.remove());

    Effect::new(move |_| {
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            if is_resizing.get() {
                let _ = body.style().set_property("cursor", "col-resize");
            } else {
                let _ = body.style().remove_property("cursor");
            }
        }
    });

    let handle_file = move |ev: web_sys::Event| {
        let target: HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(files) = target.files()
            && let Some(file) = files.get(0)
        {
            let fname = file.name();
            spawn_local(async move {
                set_status.set("Loading & Verifying...".to_string());
                match read_file_to_vec(&file).await {
                    Ok(vec) => {
                        let data_arc: Arc<[u8]> = Arc::from(vec);

                        match BBFReader::new(data_arc) {
                            Ok(r) => {
                                let assets = r.assets();
                                let mut bad = 0;
                                for (i, asset) in assets.iter().enumerate() {
                                    if let Ok(data) = r.get_asset(i as u64) {
                                        let hash = xxhash_rust::xxh3::xxh3_128(data);
                                        let hash_lo = (hash & 0xFFFF_FFFF_FFFF_FFFF) as u64;
                                        let hash_hi = (hash >> 64) as u64;
                                        if hash_lo != asset.asset_hash[0].get()
                                            || hash_hi != asset.asset_hash[1].get()
                                        {
                                            bad += 1;
                                        }
                                    } else {
                                        bad += 1;
                                    }
                                }

                                if bad == 0 {
                                    set_status.set("Integrity: OK".to_string());
                                } else {
                                    set_status.set(format!("Integrity: {bad} CORRUPT"));
                                }

                                set_book.set(Some(LoadedBook {
                                    name: fname,
                                    reader: Arc::new(r),
                                }));
                                set_page_idx.set(0);
                            }
                            Err(e) => set_status.set(format!("Invalid BBF: {e:?}")),
                        }
                    }
                    Err(_) => set_status.set("Read error".to_string()),
                }
            });
        }
    };

    Effect::new(move |_| {
        if let Some(bk) = book.get() {
            let idx = page_idx.get();
            let pages = bk.reader.pages();
            if (idx as usize) < pages.len() {
                let page = &pages[idx as usize];
                let asset_idx = page.asset_index.get();
                if let Ok(asset_data) = bk.reader.get_asset(asset_idx) {
                    let assets = bk.reader.assets();
                    let asset_entry = &assets[asset_idx as usize];
                    let mime = BBFMediaType::from(asset_entry.type_).as_extension();

                    let mime_str = match mime {
                        ".png" => "image/png",
                        ".jpg" | ".jpeg" => "image/jpeg",
                        ".avif" => "image/avif",
                        ".webp" => "image/webp",
                        _ => "application/octet-stream",
                    };

                    let array = js_sys::Array::new();
                    let u8arr = js_sys::Uint8Array::from(asset_data);
                    array.push(&u8arr.buffer());

                    let bag = web_sys::BlobPropertyBag::new();
                    bag.set_type(mime_str);

                    if let Ok(blob) =
                        web_sys::Blob::new_with_blob_sequence_and_options(&array, &bag)
                        && let Ok(url) = Url::create_object_url_with_blob(&blob)
                    {
                        let old = img_url.get_untracked();
                        if !old.is_empty() {
                            let _ = Url::revoke_object_url(&old);
                        }
                        set_img_url.set(url);
                    }
                }
            }
        }
    });

    let next_page_logic = move || {
        if let Some(bk) = book.get() {
            let max = bk.reader.pages().len() as u32;
            if page_idx.get() + 1 < max {
                set_page_idx.update(|i| *i += 1);
            }
        }
    };

    let prev_page_logic = move || {
        if page_idx.get() > 0 {
            set_page_idx.update(|i| *i -= 1);
        }
    };

    view! {
        <div class=reader_css::CONTAINER>
            <Show when=move || book.get().is_some() fallback=move || view! {
                <label class=reader_css::EMPTY_STATE>
                    <div class=reader_css::EMPTY_ICON>"📖"</div>
                    <div class=reader_css::EMPTY_TEXT>"Select a BBF file to begin reading."</div>
                    <input type="file" accept=".bbf" on:change=handle_file class="hidden" style="display:none" />
                </label>
            }>
                <div class=reader_css::MAIN_CONTENT>
                    <div
                        class=reader_css::SIDEBAR
                        style=move || format!("width: {}px", sidebar_width.get())
                    >
                        <div class=reader_css::SIDEBAR_CONTROLS>
                             <label class=reader_css::SIDEBAR_BTN>
                                "Open New File"
                                <input type="file" accept=".bbf" on:change=handle_file class="hidden" style="display:none" />
                            </label>
                            <div class=reader_css::STATUS>{move || status.get()}</div>
                        </div>

                        <div class=reader_css::SIDEBAR_HEADER>"Sections"</div>
                        <ul class=reader_css::SIDEBAR_LIST>
                            {move || {
                                book.get().map(|bk| {
                                    let reader = bk.reader;
                                    let reader_for_closure = reader.clone();

                                    reader.sections().iter().map(move |s| {
                                        let title = reader_for_closure.get_string(s.section_title_offset.get()).unwrap_or("?").to_string();
                                        let page = s.section_start_index.get() as u32;
                                        let is_active = page_idx.get() >= page;

                                        view! {
                                            <li
                                                class=if is_active {
                                                    format!("{} {}", reader_css::SECTION_ITEM, reader_css::ACTIVE)
                                                } else {
                                                    reader_css::SECTION_ITEM.to_string()
                                                }
                                                on:click=move |_| set_page_idx.set(page)
                                            >
                                                <div class=reader_css::SECTION_TITLE>{title}</div>
                                                <div class=reader_css::SECTION_PAGE>"Page " {page + 1}</div>
                                            </li>
                                        }
                                    }).collect_view()
                                })
                            }}
                        </ul>

                         <div class=format!("{} {}", reader_css::SIDEBAR_HEADER, reader_css::SIDEBAR_HEADER_META)>"Metadata"</div>

                         <ul class=reader_css::META_LIST>
                             {move || {
                                book.get().map(|bk| {
                                    let reader = bk.reader;
                                    let reader_for_closure = reader.clone();

                                    reader.metadata().iter().map(move |m| {
                                        let k = reader_for_closure.get_string(m.key_offset.get()).unwrap_or("?").to_string();
                                        let v = reader_for_closure.get_string(m.value_offset.get()).unwrap_or("?").to_string();
                                        view! {
                                            <li class=reader_css::META_ITEM>
                                                <span class=reader_css::META_KEY>{k}</span>
                                                <span class=reader_css::META_VAL>{v}</span>
                                            </li>
                                        }
                                    }).collect_view()
                                })
                            }}
                         </ul>
                    </div>

                    <div
                        class=move || if is_resizing.get() {
                            format!("{} {}", reader_css::RESIZER, reader_css::RESIZER_ACTIVE)
                        } else {
                            reader_css::RESIZER.to_string()
                        }
                        on:mousedown=start_resize
                    ></div>

                    <div class=reader_css::VIEWER_AREA>
                        <div
                            class=reader_css::IMAGE_CONTAINER
                            on:click=move |ev| {
                                 let width = web_sys::window().unwrap().inner_width().unwrap().as_f64().unwrap();
                                 let x = f64::from(ev.client_x());
                                 if x > width / 2.0 { next_page_logic(); } else { prev_page_logic(); }
                            }
                        >
                            <img src=move || img_url.get() class=reader_css::PAGE_IMAGE />
                        </div>

                        <div class=reader_css::CONTROLS>
                             <button on:click=move |_| prev_page_logic() class=reader_css::NAV_BTN>
                                "Previous"
                             </button>

                             <span class=reader_css::PAGE_COUNTER>
                                "Page " <span class=reader_css::PAGE_NUMBER>{move || page_idx.get() + 1}</span>
                             </span>

                             <button on:click=move |_| next_page_logic() class=reader_css::NAV_BTN>
                                "Next"
                             </button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
