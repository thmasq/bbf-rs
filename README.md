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

## License

Distributed under the MIT License. See `LICENSE` for more information.
