A Rust port of the **[Bound Book Format (BBF)](https://github.com/ef1500/libbbf)** library.

> **Credits:** This project is a direct port of the original `libbbf` by [ef1500](https://github.com/ef1500). It might differ slightly in failure modes.

## WebAssembly Usage

To use the BBF reader in your web projects:

1. Download `bbf-wasm-web.tar.gz` from the latest Release.
2. Extract the `pkg/` directory into your project.

### Synchronous usage

This is the simplest way to load and read a `.bbf` file if you don't mind waiting for the entire file to download before rendering anything.

```javascript
import init, { WasmBBFStreamer } from './pkg/bbf_wasm.js';

async function loadBook() {
    // 1. Initialize the WebAssembly module
    await init();

    // 2. Fetch the entire BBF file
    const response = await fetch("my_book.bbf");
    const buffer = await response.arrayBuffer();

    // 3. Feed it to the Reader
    const reader = new WasmBBFStreamer();
    reader.append_chunk(new Uint8Array(buffer));

    if (reader.is_ready()) {
        console.log(`Loaded book with ${reader.page_count} pages!`);

        // 4. Extract Page 1 and determine its MIME type
        const pageData = reader.get_page(0); // Returns a Uint8Array
        const mimeType = reader.get_page_mime(0); // e.g. "image/png"

        // 5. Render it
        const blob = new Blob([pageData], { type: mimeType });
        const url = URL.createObjectURL(blob);
        document.getElementById("my-image").src = url;
    }
}

loadBook();
```

### Progressive Streaming

If your BBF file was built with the Petrification flag, you can use the Javascript Fetch API to stream the file. The Wasm reader will instantly parse the index from the first few kilobytes, allowing you to decode and display pages while
the rest of the file continues downloading in the background.

```javascript
import init, { WasmBBFStreamer } from './pkg/bbf_wasm.js';

async function streamBookAndDecodeFast() {
    await init();
    const streamer = new WasmBBFStreamer();

    const targetPage = 0; // Page we want to display as soon as it is ready
    let pageRendered = false;

    // Start a streaming fetch request
    const response = await fetch("petrified_book.bbf");
    const reader = response.body.getReader();

    while (true) {
        const { done, value } = await reader.read();

        if (value) {
            // Push the downloaded chunk into WebAssembly memory
            streamer.append_chunk(value);

            // 1. Check if the table of contents has arrived
            if (streamer.is_ready() && !pageRendered) {

                // 2. Check if the target page's bytes have arrived!
                if (streamer.is_page_ready(targetPage)) {
			console.log(`Page ${targetPage} decoded BEFORE file finished downloading!`);
			
			const pageData = streamer.get_page(targetPage);
			const mimeType = streamer.get_page_mime(targetPage);
			
			const blob = new Blob([pageData], { type: mimeType });
			document.getElementById("my-image").src = URL.createObjectURL(blob);
			
			pageRendered = true;
                }
            }
        }

        if (done) {
            console.log("File finished downloading!");
            break;
        }
    }
}

streamBookAndDecodeFast();
```

(Note: If a file is not petrified, the streaming example will still work, but is_ready() will seamlessly fall back to waiting for the final chunk before it returns true).

### Ranged Loading (Out-of-Order)

For very large petrified files, downloading the entire archive sequentially can consume too much RAM or bandwidth. Because libbbf stores image assets as raw bytes, Javascript can completely bypass WebAssembly for decoding once the index is parsed.

By fetching just the index and then aborting the main stream, you can use HTTP Range requests to dynamically fetch specific pages out-of-order. This is similar to how video streaming works.

```javascript
import init, { WasmBBFStreamer } from './pkg/bbf_wasm.js';

async function loadBookWithRanges() {
    await init();
    const streamer = new WasmBBFStreamer();

    // Use an AbortController so you can stop the download
    // immediately after receiving the table of contents.
    const abortController = new AbortController();
    const url = "petrified_book.bbf";

    try {
        const response = await fetch(url, { signal: abortController.signal });
        const reader = response.body.getReader();

        while (true) {
            const { done, value } = await reader.read();
            if (value) streamer.append_chunk(value);

            // The moment the index is parsed, stop downloading
            if (streamer.is_ready()) {
                abortController.abort();

                // Now you can fetch any page you want
                await fetchSpecificPage(streamer, url, 0); // Load first page
                await fetchSpecificPage(streamer, url, 50); // Instantly jump to page 51
                break;
            }
            if (done) break;
        }
    } catch (err) {
        if (err.name !== 'AbortError') console.error(err);
    }
}

async function fetchSpecificPage(streamer, url, pageIndex) {
    // 1. Ask Wasm for the absolute file offsets of this page
    const info = streamer.get_page_info(pageIndex);
    const rangeEnd = info.offset + info.size - 1;

    // 2. Fetch exactly those bytes from the server
    const headers = { 'Range': `bytes=${info.offset}-${rangeEnd}` };
    const response = await fetch(url, { headers });

    // 3. Render the raw bytes directly using the browser
    const rawBlob = await response.blob();
    const typedBlob = new Blob([rawBlob], { type: info.mimeType });

    const img = document.createElement("img");
    img.src = URL.createObjectURL(typedBlob);
    document.body.appendChild(img);
}

loadBookWithRanges();
```

(Note: The web server hosting the .bbf files must support the HTTP Range header for this feature to work).

## License

Distributed under the MIT License. See `LICENSE` for more information.
