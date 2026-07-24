use crate::utils::{download_blob, read_file_to_vec};
use bbf::{BBFBuilder, BBFMediaType};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_styling::inline_style_sheet;
use std::io::Cursor;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, KeyboardEvent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendFile(pub web_sys::File);

unsafe impl Send for SendFile {}
unsafe impl Sync for SendFile {}

impl std::ops::Deref for SendFile {
    type Target = web_sys::File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
enum BuilderEntry {
    File {
        id: usize,
        file: SendFile,
        name: String,
    },
    Section {
        id: usize,
        name: RwSignal<String>,
        parent: Option<String>,
    },
}

impl BuilderEntry {
    const fn id(&self) -> usize {
        match self {
            Self::File { id, .. } | Self::Section { id, .. } => *id,
        }
    }

    fn name(&self) -> String {
        match self {
            Self::File { name, .. } => name.clone(),
            Self::Section { name, .. } => name.get(),
        }
    }

    const fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct MetaEntry {
    id: usize,
    key: String,
    value: String,
}

#[allow(clippy::too_many_lines)]
#[component]
pub fn Builder() -> impl IntoView {
    let (entries, set_entries) = signal(Vec::<BuilderEntry>::new());
    let (metadata, set_metadata) = signal(Vec::<MetaEntry>::new());
    let (status, set_status) = signal(String::new());
    let (petrify, set_petrify) = signal(false);

    let (editing_id, set_editing_id) = signal(Option::<usize>::None);
    let (drag_id, set_drag_id) = signal(Option::<usize>::None);

    let (floating_entry, set_floating_entry) = signal(Option::<BuilderEntry>::None);
    let (mouse_pos, set_mouse_pos) = signal((0.0, 0.0));

    let next_id = RwSignal::new(0_usize);
    let get_id = move || {
        next_id.update(|n| *n += 1);
        next_id.get_untracked()
    };

    inline_style_sheet! {
        builder_css,
        "builder",

        .container {
            padding: 1.5rem;
            height: 100%;
            overflow-y: auto;
            color: #e2e8f0; /* text-slate-200 */
        }

        .title {
            font-size: 1.5rem;
            font-weight: 700;
            margin-bottom: 1.5rem;
            color: #818cf8; /* text-indigo-400 */
        }

        /* Control Panel (File Input & Buttons) */
        .control-panel {
            background-color: #0f172a; /* bg-slate-900 */
            border: 1px solid #334155; /* border-slate-700 */
            border-radius: 0.75rem;
            padding: 1.5rem;
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 8px 10px -6px rgba(0, 0, 0, 0.1);
            margin-bottom: 1.5rem;
        }

        .input-label {
            display: block;
            margin-bottom: 0.5rem;
            font-size: 0.875rem;
            font-weight: 500;
            color: #cbd5e1; /* text-slate-300 */
        }

        /* Styling the File Input */
        .file-input {
            display: block;
            width: 100%;
            font-size: 0.875rem;
            color: #94a3b8; /* text-slate-400 */
            cursor: pointer;
        }
        /* Target the file button specifically */
        .file-input::file-selector-button {
            margin-right: 1rem;
            padding: 0.5rem 1rem;
            border-radius: 9999px;
            border: 0;
            font-size: 0.875rem;
            font-weight: 600;
            background-color: #312e81; /* bg-indigo-900 */
            color: #a5b4fc; /* text-indigo-300 */
            cursor: pointer;
            transition: background-color 0.2s;
        }
        .file-input::file-selector-button:hover {
            background-color: #3730a3; /* bg-indigo-800 */
        }

        .btn-group {
            display: flex;
            gap: 1rem;
            margin-top: 1rem;
        }

        .action-btn {
            background-color: #1e293b; /* bg-slate-800 */
            border: 1px solid #475569; /* border-slate-600 */
            padding: 0.5rem 1rem;
            border-radius: 0.5rem;
            transition: background-color 0.2s;
            display: flex;
            align-items: center;
            gap: 0.5rem;
            cursor: pointer;
        }
        .action-btn:hover { background-color: #334155; }
        .text-indigo { color: #a5b4fc; }
        .text-emerald { color: #34d399; }

        /* Main Columns Layout */
        .columns-wrapper {
            display: flex;
            flex-direction: column;
            gap: 1.5rem;
        }
        @media (min-width: 768px) {
            .columns-wrapper { flex-direction: row; }
        }

        .panel {
            width: 100%;
            background-color: #0f172a;
            border: 1px solid #334155;
            border-radius: 0.75rem;
            padding: 1rem;
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1);
        }
        @media (min-width: 768px) {
            .panel { width: 50%; }
        }

        .panel-header {
            font-weight: 700;
            margin-bottom: 1rem;
            color: #e2e8f0;
            border-bottom: 1px solid #334155;
            padding-bottom: 0.5rem;
        }

        /* List Items (Draggable) */
        .list-container {
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
            max-height: 500px;
            overflow-y: auto;
            min-height: 100px;
            padding-right: 0.5rem;
        }

        .list-item {
            padding: 0.75rem;
            border-radius: 0.5rem;
            border: 1px solid #334155; /* Default border */
            background-color: #1e293b; /* Default bg */
            display: flex;
            align-items: center;
            justify-content: space-between;
            cursor: move;
            transition: all 0.2s;
            user-select: none;
            position: relative;
        }

        .list-item-content {
            display: flex;
            align-items: center;
            flex: 1;
            gap: 0.75rem;
            min-width: 0;
        }

        /* Dragging State */
        .list-item[data-dragging="true"] {
            background-color: #334155;
            opacity: 0.5;
        }

        /* Editing State */
        .list-item[data-editing="true"] {
            border-color: #6366f1; /* indigo-500 */
        }

        .item-icon { font-size: 1.25rem; flex-shrink: 0; }
        .item-text {
            color: #cbd5e1;
            display: block;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }

        /* Inline Input for renaming */
        .inline-input {
            background-color: #0f172a;
            color: white;
            padding: 0.25rem;
            border: none;
            border-bottom: 1px solid #6366f1;
            outline: none;
            width: 100%;
        }

        /* Remove Button */
        .remove-btn {
            color: #64748b;
            padding: 0.5rem;
            margin-left: 0.5rem;
            transition: all 0.2s;
            cursor: pointer;
            background: none;
            border: none;
            opacity: 0.7;
        }

        .remove-btn:hover {
            opacity: 1;
            color: #f87171;
            background-color: rgba(255, 255, 255, 0.05);
            border-radius: 0.25rem;
        }

        /* Metadata Row */
        .meta-row { display: flex; gap: 0.5rem; align-items: center; }
        .meta-input {
            background-color: #1e293b;
            border: 1px solid #475569;
            border-radius: 0.25rem;
            padding: 0.5rem;
            color: #e2e8f0;
            width: 100%;
        }
        .meta-input:focus { outline: 2px solid #6366f1; border-color: transparent; }

        /* Floating "Ghost" Element */
        .ghost-cursor {
            position: fixed;
            z-index: 50;
            pointer-events: none;
            padding: 0.75rem;
            border-radius: 0.5rem;
            border: 1px solid #6366f1;
            background-color: #1e293b;
            box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.25);
            opacity: 0.9;
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }

        /* Bottom Bar */
        .bottom-bar {
            margin-top: 2rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            background-color: #0f172a;
            border: 1px solid #334155;
            border-radius: 0.75rem;
            padding: 1rem;
        }

        .status-text { color: #818cf8; font-family: monospace; }

        .compile-btn {
            background-color: #4f46e5;
            color: white;
            padding: 0.75rem 2rem;
            border-radius: 0.5rem;
            font-size: 1.125rem;
            font-weight: 700;
            border: none;
            cursor: pointer;
            transition: transform 0.1s, box-shadow 0.2s;
            box-shadow: 0 0 20px rgba(79, 70, 229, 0.4);
        }
        .compile-btn:hover {
            background-color: #6366f1;
            box-shadow: 0 0 30px rgba(79, 70, 229, 0.6);
            transform: translateY(-2px);
        }

        .empty-text {
            text-align: center;
            padding: 2rem 0;
            color: #64748b;
            font-style: italic;
        }
    }

    let handle = window_event_listener(leptos::ev::mousemove, move |ev| {
        set_mouse_pos.set((f64::from(ev.client_x()), f64::from(ev.client_y())));
    });

    on_cleanup(move || handle.remove());

    let handle_click = window_event_listener(leptos::ev::click, move |_| {
        if floating_entry.get_untracked().is_some() {
            set_floating_entry.set(None);
        }
    });

    on_cleanup(move || handle_click.remove());

    let handle_files = move |ev: web_sys::Event| {
        let target: HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(files) = target.files() {
            let mut new_entries = Vec::new();
            for i in 0..files.length() {
                if let Some(file) = files.get(i) {
                    new_entries.push(BuilderEntry::File {
                        id: get_id(),
                        name: file.name(),
                        file: SendFile(file),
                    });
                }
            }
            set_entries.update(move |e: &mut Vec<BuilderEntry>| e.extend(new_entries));
        }
    };

    let add_section = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        let id = get_id();
        let entry = BuilderEntry::Section {
            id,
            name: RwSignal::new("New Section".to_string()),
            parent: None,
        };
        set_floating_entry.set(Some(entry));
    };

    let add_meta = move |_| {
        let id = get_id();
        set_metadata.update(move |m: &mut Vec<MetaEntry>| {
            m.push(MetaEntry {
                id,
                key: String::new(),
                value: String::new(),
            });
        });
    };

    let remove_entry = move |id: usize| {
        set_entries.update(|e| e.retain(|x| x.id() != id));
    };

    let handle_drag_start = move |id: usize| {
        set_drag_id.set(Some(id));
    };

    let handle_drop = move |target_id: usize| {
        if let Some(dragged) = drag_id.get()
            && dragged != target_id
        {
            set_entries.update(|list| {
                if let Some(from_idx) = list.iter().position(|e| e.id() == dragged)
                    && let Some(to_idx) = list.iter().position(|e| e.id() == target_id)
                {
                    let item = list.remove(from_idx);
                    list.insert(to_idx, item);
                }
            });
        }
        set_drag_id.set(None);
    };

    let handle_container_click = move |ev: web_sys::MouseEvent| {
        if let Some(entry) = floating_entry.get() {
            ev.stop_propagation();

            let target_div = ev
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                .expect("Handler attached to HtmlElement");

            let children = target_div.children();
            let len = children.length();
            let mouse_y = f64::from(ev.client_y());

            let mut insert_idx = len as usize;

            for i in 0..len {
                if let Some(child) = children.item(i) {
                    let rect = child.get_bounding_client_rect();
                    let mid = rect.top() + (rect.height() / 2.0);
                    if mouse_y < mid {
                        insert_idx = i as usize;
                        break;
                    }
                }
            }

            set_entries.update(|list| {
                if insert_idx >= list.len() {
                    list.push(entry);
                } else {
                    list.insert(insert_idx, entry);
                }
            });
            set_floating_entry.set(None);
        }
    };

    let compile = move |_| {
        spawn_local(async move {
            set_status.set("Reading files...".to_string());
            let current_entries = entries.get();
            let current_meta = metadata.get();

            let mut cursor = Cursor::new(Vec::new());

            let mut builder = match BBFBuilder::new(&mut cursor) {
                Ok(b) => b,
                Err(err) => {
                    set_status.set(format!("Error initializing builder: {err:?}"));
                    return;
                }
            };

            let mut page_count = 0;

            for entry in current_entries {
                match entry {
                    BuilderEntry::File { file, name, .. } => {
                        if let Ok(data) = read_file_to_vec(&file).await {
                            let ext = std::path::Path::new(&name)
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|e| format!(".{e}"))
                                .unwrap_or_default();

                            let media_type = BBFMediaType::from_extension(&ext);
                            if let Err(err) = builder.add_page(&data, media_type, 0) {
                                set_status.set(format!("Error adding page: {err:?}"));
                                return;
                            }
                            page_count += 1;
                        } else {
                            set_status.set("Failed to read file".to_string());
                            return;
                        }
                    }
                    BuilderEntry::Section { name, .. } => {
                        builder.add_section(&name.get(), page_count, None);
                    }
                }
            }

            for meta in current_meta {
                builder.add_metadata(&meta.key, &meta.value);
            }

            if let Err(err) = builder.finalize() {
                set_status.set(format!("Error finalizing: {err:?}"));
                return;
            }

            if petrify.get_untracked() {
                set_status.set("Petrifying archive...".to_string());
                if let Err(err) = bbf::builder::petrify(&mut cursor) {
                    set_status.set(format!("Error petrifying: {err:?}"));
                    return;
                }
            }

            set_status.set("Download starting...".to_string());
            let _ = download_blob(
                cursor.get_ref(),
                "web_generated.bbf",
                "application/octet-stream",
            );
            set_status.set("Done!".to_string());
        });
    };

    view! {
        <div class=builder_css::CONTAINER>
            <Show when=move || floating_entry.get().is_some()>
                <div
                    class=builder_css::GHOST_CURSOR
                    style=move || format!("left: {}px; top: {}px; transform: translate(10px, 10px);", mouse_pos.get().0, mouse_pos.get().1)
                >
                      <span class="text-xl">"🔖"</span>
                      <span class="font-bold text-slate-300">"Place New Section..."</span>
                </div>
            </Show>

            <h2 class=builder_css::TITLE>"BBF Builder Mode"</h2>

            <div class=builder_css::CONTROL_PANEL>
                <div class="mb-4">
                    <label class=builder_css::INPUT_LABEL>"Add Files"</label>
                    <input
                        type="file"
                        multiple
                        on:change=handle_files
                        class=builder_css::FILE_INPUT
                    />
                </div>

                <div class=builder_css::BTN_GROUP>
                    <button
                        on:click=add_section
                        class=builder_css::ACTION_BTN
                    >
                         <span class=builder_css::TEXT_INDIGO>"Add Section Marker"</span>
                    </button>
                    <button
                        on:click=add_meta
                        class=builder_css::ACTION_BTN
                    >
                         <span class=builder_css::TEXT_EMERALD>"Add Metadata"</span>
                    </button>
                </div>
            </div>

            <div class=builder_css::COLUMNS_WRAPPER>
                <div class=builder_css::PANEL>
                    <h3 class=builder_css::PANEL_HEADER>"Content Order"</h3>

                    <div
                        class=builder_css::LIST_CONTAINER
                        on:click=handle_container_click
                    >
                        <For
                            each=move || entries.get()
                            key=|e| e.id()
                            children=move |e| {
                                let id = e.id();
                                let is_section = e.is_section();

                                let is_editing = move || editing_id.get() == Some(id);
                                let is_dragging = move || drag_id.get() == Some(id);

                                view! {
                                    <div
                                        class=builder_css::LIST_ITEM
                                        attr:data-dragging=move || is_dragging().to_string()
                                        attr:data-editing=move || is_editing().to_string()

                                        draggable=move || if is_editing() { "false" } else { "true" }
                                        on:dragstart=move |_| handle_drag_start(id)
                                        on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                                        on:drop=move |ev: web_sys::DragEvent| {
                                            ev.prevent_default();
                                            handle_drop(id);
                                        }

                                        on:dblclick=move |_| {
                                            if is_section {
                                                set_editing_id.set(Some(id));
                                            }
                                        }
                                    >
                                        <div class=builder_css::LIST_ITEM_CONTENT>
                                            <span class=builder_css::ITEM_ICON>
                                                {match e {
                                                    BuilderEntry::File { .. } => "📄",
                                                    BuilderEntry::Section { .. } => "🔖",
                                                }}
                                            </span>

                                            <div class="flex-1 min-w-0">
                                            {move || {
                                                if is_editing() {
                                                    if let BuilderEntry::Section { name, .. } = e {
                                                        view! {
                                                            <input
                                                                type="text"
                                                                class=builder_css::INLINE_INPUT
                                                                prop:value=move || name.get()
                                                                autofocus
                                                                on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
                                                                on:keydown=move |ev: KeyboardEvent| {
                                                                    if ev.key() == "Enter" {
                                                                        ev.prevent_default();
                                                                        let val = event_target_value(&ev);
                                                                        name.set(val);
                                                                        set_editing_id.set(None);
                                                                    } else if ev.key() == "Escape" {
                                                                        set_editing_id.set(None);
                                                                    }
                                                                }
                                                                on:blur=move |_| set_editing_id.set(None)
                                                            />
                                                        }.into_any()
                                                    } else {
                                                        let _: () = view! {};
                                                        ().into_any()
                                                    }
                                                } else {
                                                    let display_name = e.name();
                                                    view! {
                                                        <span class=builder_css::ITEM_TEXT title=display_name>
                                                            {display_name.clone()}
                                                        </span>
                                                    }.into_any()
                                                }
                                            }}
                                            </div>
                                        </div>

                                        <button
                                            class=builder_css::REMOVE_BTN
                                            title="Remove"
                                            on:click=move |ev: web_sys::MouseEvent| {
                                                ev.stop_propagation();
                                                remove_entry(id);
                                            }
                                        >
                                            "✕"
                                        </button>
                                    </div>
                                }
                            }
                        />
                         <Show when=move || entries.get().is_empty()>
                            <div class=builder_css::EMPTY_TEXT>"No files added yet."</div>
                         </Show>
                    </div>
                </div>

                <div class=builder_css::PANEL>
                    <h3 class=builder_css::PANEL_HEADER>"Metadata"</h3>
                      <div class="space-y-2">
                          <For
                            each=move || metadata.get()
                            key=|m| m.id
                            children=move |m| {
                                view! {
                                    <div class=builder_css::META_ROW>
                                        <input class=builder_css::META_INPUT style="width: 33%"
                                               placeholder="Key"
                                               prop:value=m.key
                                               on:input=move |ev| {
                                                   let val = event_target_value(&ev);
                                                   set_metadata.update(|list| {
                                                       if let Some(item) = list.iter_mut().find(|i| i.id == m.id) {
                                                           item.key = val;
                                                       }
                                                   });
                                               }
                                        />
                                        <input class=builder_css::META_INPUT
                                               placeholder="Value"
                                               prop:value=m.value
                                               on:input=move |ev| {
                                                   let val = event_target_value(&ev);
                                                   set_metadata.update(|list| {
                                                       if let Some(item) = list.iter_mut().find(|i| i.id == m.id) {
                                                           item.value = val;
                                                       }
                                                   });
                                               }
                                        />
                                        <button
                                            class=builder_css::REMOVE_BTN
                                            on:click=move |_| set_metadata.update(|list| list.retain(|x| x.id != m.id))
                                        >
                                            "✕"
                                        </button>
                                    </div>
                                }
                            }
                        />
                        <Show when=move || metadata.get().is_empty()>
                            <div class=builder_css::EMPTY_TEXT>"No metadata."</div>
                         </Show>
                    </div>
                </div>
            </div>

            <div class=builder_css::BOTTOM_BAR>
                <div class=builder_css::STATUS_TEXT>{status}</div>
                <div style="display: flex; gap: 1rem; align-items: center;">
                    <label style="color: #cbd5e1; cursor: pointer; display: flex; align-items: center; gap: 0.5rem;">
                        <input
                            type="checkbox"
                            prop:checked=move || petrify.get()
                            on:change=move |ev| set_petrify.set(event_target_checked(&ev))
                        />
                        <span>"Petrify Archive"</span>
                    </label>
                    <button on:click=compile class=builder_css::COMPILE_BTN >
                        "Compile & Download .bbf"
                    </button>
                </div>
            </div>
        </div>
    }
}
