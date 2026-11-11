# WASM fallback fonts

The WASM host uses bundled Roboto first. An app can supply font or font collection bytes as a fallback before any text engine is initialized:

```js
const host = await connectWasm(wasmBytes);
host.setFallbackFont(fontBytes); // Uint8Array
```

Supply bytes for a font you have the right to use and distribute. The caller owns its licensing. The host copies the bytes and validates them; invalid bytes throw an error and do not freeze configuration. A valid call can be made once. Text layout or paint can initialize text first, after which the setter throws. Configure the font before rendering or any other operation that initializes text.

Configuration belongs to the WASM module instance for its lifetime, shared by its trees and text engines. `reset()` clears the scene and restarts node IDs; it does not clear font configuration or undo text initialization. Instantiate a fresh module to start over. Without a supplied font, the existing bundled default is unchanged. Supplying Roboto again does not add CJK coverage.

Node WASM smoke tests do not establish browser compatibility.
