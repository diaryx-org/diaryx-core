/**
 * Web Worker for pandoc WASM conversion.
 *
 * Bypasses wasm-pandoc's browser entry point (which uses `import("./pandoc.wasm")`
 * that Vite can't handle) and instead loads the WASM binary via fetch + URL,
 * then uses wasm-pandoc's core.js for instantiation.
 *
 * Messages IN:
 *   { type: 'init' }
 *   { type: 'convert', id: number, from: string, to: string, content: string, resources?: Record<string, Blob>, standalone?: boolean }
 *
 * Messages OUT:
 *   { type: 'ready' }
 *   { type: 'result', id: number, output: any }
 *   { type: 'error', id: number, error: string }
 */

// Use ?url to get static asset URLs without triggering Vite's WASM module processing.
// These bypass the package "exports" field via Vite's file-based resolution.
// @ts-expect-error - Vite ?url import
import wasmUrl from '/node_modules/wasm-pandoc/src/pandoc.wasm?url';

let convertFn: ((options: any, stdin: string | null, files: Record<string, any>) => Promise<any>) | null = null;
let pandocReady = false;

async function initPandoc() {
  // Dynamically import core.js (the WASI shim + instantiation logic)
  // Use absolute path to bypass strict "exports" resolution
  // @ts-expect-error - direct node_modules import
  const { createPandocInstance } = await import('/node_modules/wasm-pandoc/src/core.js');

  // Fetch the WASM binary as an ArrayBuffer
  const response = await fetch(wasmUrl);
  const wasmBinary = await response.arrayBuffer();

  // Use wasm-pandoc's core to create the instance (handles WASI shim setup)
  const instance = await createPandocInstance(wasmBinary);
  convertFn = instance.convert;
  pandocReady = true;
  self.postMessage({ type: 'ready' });
}

self.onmessage = async (e: MessageEvent) => {
  const { type, id, ...params } = e.data;

  if (type === 'init') {
    try {
      await initPandoc();
    } catch (err) {
      self.postMessage({ type: 'error', id: 0, error: `Failed to load pandoc WASM: ${err}` });
    }
    return;
  }

  if (type === 'convert') {
    if (!pandocReady) {
      try {
        await initPandoc();
      } catch (err) {
        self.postMessage({ type: 'error', id, error: `Failed to load pandoc WASM: ${err}` });
        return;
      }
    }

    try {
      const options: Record<string, any> = {
        from: params.from ?? 'markdown',
        to: params.to,
      };
      if (params.standalone !== false) {
        options.standalone = true;
      }

      // For binary formats, set an output-file so pandoc writes to the virtual filesystem
      // instead of stdout. Without this, binary formats produce empty output.
      const binaryFormats = ['docx', 'epub', 'odt', 'pdf'];
      const isBinary = binaryFormats.includes(params.to);
      const outputFilename = isBinary ? `output.${params.to}` : undefined;
      if (outputFilename) {
        options['output-file'] = outputFilename;
      }

      const result = await convertFn!(options, params.content ?? null, params.resources ?? {});

      // Convert any Blob values in result.files to Uint8Array for postMessage transfer
      const serializedFiles: Record<string, Uint8Array> = {};
      if (result.files) {
        for (const [name, value] of Object.entries(result.files)) {
          if (value instanceof Blob) {
            serializedFiles[name] = new Uint8Array(await (value as Blob).arrayBuffer());
          } else if (value instanceof Uint8Array) {
            serializedFiles[name] = value;
          }
        }
      }

      self.postMessage({
        type: 'result',
        id,
        output: {
          stdout: result.stdout ?? '',
          stderr: result.stderr ?? '',
          files: serializedFiles,
          outputFilename,
        },
      });
    } catch (err) {
      self.postMessage({ type: 'error', id, error: `Pandoc conversion failed: ${err}` });
    }
  }
};
