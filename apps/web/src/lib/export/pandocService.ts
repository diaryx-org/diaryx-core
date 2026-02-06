/**
 * Pandoc WASM service for the web app.
 *
 * Manages a Web Worker that lazily loads wasm-pandoc (~54 MB).
 * Provides a promise-based API for converting markdown to various formats.
 */

/** Supported export format identifiers. */
export type ExportFormat =
  | 'markdown'
  | 'html'
  | 'docx'
  | 'epub'
  | 'pdf'
  | 'latex'
  | 'odt'
  | 'rst';

/** Metadata for a supported export format. */
export interface FormatInfo {
  id: ExportFormat;
  label: string;
  extension: string;
  /** Whether the output is a binary file (docx, epub, pdf, odt). */
  binary: boolean;
  /** Whether this format requires wasm-pandoc (false for markdown and html). */
  requiresPandoc: boolean;
}

/**
 * All supported export formats with metadata.
 *
 * PDF uses html2pdf.js (html2canvas + jsPDF) on the web, converting via
 * the existing HTML export pipeline. It does not require wasm-pandoc.
 * CLI/Tauri use native pandoc+typst for PDF instead.
 */
export const EXPORT_FORMATS: FormatInfo[] = [
  { id: 'markdown', label: 'Markdown', extension: '.md', binary: false, requiresPandoc: false },
  { id: 'html', label: 'HTML', extension: '.html', binary: false, requiresPandoc: false },
  { id: 'pdf', label: 'PDF', extension: '.pdf', binary: true, requiresPandoc: false },
  { id: 'docx', label: 'Word (DOCX)', extension: '.docx', binary: true, requiresPandoc: true },
  { id: 'epub', label: 'EPUB', extension: '.epub', binary: true, requiresPandoc: true },
  { id: 'latex', label: 'LaTeX', extension: '.tex', binary: false, requiresPandoc: true },
  { id: 'odt', label: 'OpenDocument (ODT)', extension: '.odt', binary: true, requiresPandoc: true },
  { id: 'rst', label: 'reStructuredText', extension: '.rst', binary: false, requiresPandoc: true },
];

/** Get format info by id. */
export function getFormatInfo(id: ExportFormat): FormatInfo | undefined {
  return EXPORT_FORMATS.find((f) => f.id === id);
}

/** Result from a pandoc conversion. */
export interface PandocResult {
  stdout: string;
  stderr: string;
  files: Record<string, Uint8Array>;
  /** The output filename used for binary formats (e.g., "output.docx"). */
  outputFilename?: string;
}

/**
 * Service that manages a pandoc Web Worker.
 *
 * The worker is created lazily on first use — the ~54 MB WASM binary
 * is only downloaded when a pandoc format is actually requested.
 */
export class PandocService {
  private worker: Worker | null = null;
  private ready = false;
  private pendingRequests = new Map<
    number,
    { resolve: (value: PandocResult) => void; reject: (reason: Error) => void }
  >();
  private nextId = 0;
  private initPromise: Promise<void> | null = null;

  /** Ensure the worker is loaded and ready. */
  async ensureReady(): Promise<void> {
    if (this.ready) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = new Promise<void>((resolve, reject) => {
      this.worker = new Worker(new URL('./pandocWorker.ts', import.meta.url), {
        type: 'module',
      });

      const onMessage = (e: MessageEvent) => {
        const msg = e.data;

        if (msg.type === 'ready') {
          this.ready = true;
          resolve();
          return;
        }

        if (msg.type === 'error' && !this.ready) {
          // Init error
          reject(new Error(msg.error));
          return;
        }

        if (msg.type === 'result' || msg.type === 'error') {
          const pending = this.pendingRequests.get(msg.id);
          if (pending) {
            this.pendingRequests.delete(msg.id);
            if (msg.type === 'result') {
              pending.resolve(msg.output);
            } else {
              pending.reject(new Error(msg.error));
            }
          }
        }
      };

      this.worker.onmessage = onMessage;
      this.worker.onerror = (e) => {
        if (!this.ready) {
          reject(new Error(`Worker error: ${e.message}`));
        }
      };

      this.worker.postMessage({ type: 'init' });
    });

    return this.initPromise;
  }

  /**
   * Convert markdown content to the target format.
   *
   * @param content - Markdown source text.
   * @param to - Target format (e.g., 'docx', 'epub', 'pdf').
   * @param resources - Optional map of filename to binary data for embedded resources (images, etc.).
   */
  async convert(
    content: string,
    to: ExportFormat,
    resources?: Record<string, Blob | Uint8Array>,
  ): Promise<PandocResult> {
    await this.ensureReady();

    const id = this.nextId++;
    // PDF is produced via Typst in pandoc
    const pandocTo = to === 'pdf' ? 'typst' : to;

    return new Promise<PandocResult>((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
      this.worker!.postMessage({
        type: 'convert',
        id,
        from: 'markdown',
        to: pandocTo,
        content,
        resources: resources ?? {},
        standalone: true,
      });
    });
  }

  /** Terminate the worker and free resources. */
  dispose() {
    this.worker?.terminate();
    this.worker = null;
    this.ready = false;
    this.initPromise = null;
    this.pendingRequests.clear();
  }
}
