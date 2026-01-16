/**
 * Attachment Service
 *
 * Manages blob URLs for displaying attachments in the editor.
 * Handles transforming file paths to blob URLs and back.
 */

import type { Api } from '$lib/backend/api';
import heic2any from 'heic2any';

// ============================================================================
// State
// ============================================================================

// Blob URL tracking for attachments (originalPath -> blobUrl)
const blobUrlMap = new Map<string, string>();

// ============================================================================
// MIME Type Mapping
// ============================================================================

const mimeTypes: Record<string, string> = {
  // Images
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  heic: 'image/heic',
  heif: 'image/heif',
  // Documents
  pdf: 'application/pdf',
  doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xls: 'application/vnd.ms-excel',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  ppt: 'application/vnd.ms-powerpoint',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  // Text
  txt: 'text/plain',
  md: 'text/markdown',
  csv: 'text/csv',
  json: 'application/json',
  xml: 'application/xml',
  // Archives
  zip: 'application/zip',
  tar: 'application/x-tar',
  gz: 'application/gzip',
  '7z': 'application/x-7z-compressed',
  rar: 'application/vnd.rar',
};

/**
 * Get the MIME type for a file based on its extension.
 */
export function getMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return mimeTypes[ext] || 'application/octet-stream';
}

/**
 * Check if a file is a HEIC/HEIF image (Apple's format).
 */
export function isHeicFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ext === 'heic' || ext === 'heif';
}

/**
 * Convert HEIC/HEIF blob to JPEG for browser display.
 * Returns original blob if conversion fails.
 */
export async function convertHeicToJpeg(blob: Blob): Promise<Blob> {
  try {
    const result = await heic2any({
      blob,
      toType: 'image/jpeg',
      quality: 0.92,
    });
    // heic2any can return an array of blobs for multi-image HEIC files
    return Array.isArray(result) ? result[0] : result;
  } catch (e) {
    console.warn('[AttachmentService] HEIC conversion failed:', e);
    return blob;
  }
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Revoke all tracked blob URLs (cleanup).
 * Should be called when switching documents or unmounting.
 */
export function revokeBlobUrls(): void {
  for (const url of blobUrlMap.values()) {
    URL.revokeObjectURL(url);
  }
  blobUrlMap.clear();
}

/**
 * Transform attachment paths in markdown content to blob URLs for display.
 *
 * @param content - Markdown content with attachment paths
 * @param entryPath - Path to the current entry (for resolving relative paths)
 * @param api - Api instance for reading attachment data
 * @returns Content with attachment paths replaced by blob URLs
 */
export async function transformAttachmentPaths(
  content: string,
  entryPath: string,
  api: Api | null,
): Promise<string> {
  if (!api) return content;

  // Find all image references: ![alt](...) or ![alt](<...>) for paths with spaces
  const imageRegex = /!\[([^\]]*)\]\((?:<([^>]+)>|([^)]+))\)/g;
  let match;
  const replacements: { original: string; replacement: string }[] = [];

  while ((match = imageRegex.exec(content)) !== null) {
    const [fullMatch, alt] = match;
    // Angle bracket path is in group 2, regular path is in group 3
    const imagePath = match[2] || match[3];

    // Skip external URLs
    if (imagePath.startsWith('http://') || imagePath.startsWith('https://')) {
      continue;
    }

    // Skip already-transformed blob URLs
    if (imagePath.startsWith('blob:')) {
      continue;
    }

    try {
      // Try to read the attachment data
      const data = await api.getAttachmentData(entryPath, imagePath);

      // Create blob and URL
      const mimeType = getMimeType(imagePath);
      let blob = new Blob([new Uint8Array(data)], { type: mimeType });

      // Convert HEIC to JPEG for browser display
      if (isHeicFile(imagePath)) {
        blob = await convertHeicToJpeg(blob);
      }

      const blobUrl = URL.createObjectURL(blob);

      // Track for cleanup
      blobUrlMap.set(imagePath, blobUrl);

      // Queue replacement
      replacements.push({
        original: fullMatch,
        replacement: `![${alt}](${blobUrl})`,
      });
    } catch (e) {
      // Attachment not found or error - leave original path
      console.warn(`[AttachmentService] Could not load attachment: ${imagePath}`, e);
    }
  }

  // Apply replacements
  let result = content;
  for (const { original, replacement } of replacements) {
    result = result.replace(original, replacement);
  }

  return result;
}

/**
 * Reverse-transform blob URLs back to attachment paths (for saving).
 * Wraps paths with spaces in angle brackets for CommonMark compatibility.
 *
 * @param content - Markdown content with blob URLs
 * @returns Content with blob URLs replaced by original attachment paths
 */
export function reverseBlobUrlsToAttachmentPaths(content: string): string {
  let result = content;

  // Iterate through blobUrlMap (originalPath -> blobUrl) and replace blob URLs with original paths
  for (const [originalPath, blobUrl] of blobUrlMap.entries()) {
    // Wrap path in angle brackets if it contains spaces (CommonMark spec)
    const pathToUse = originalPath.includes(' ')
      ? `<${originalPath}>`
      : originalPath;
    // Replace all occurrences of the blob URL with the original path
    result = result.replaceAll(blobUrl, pathToUse);
  }

  return result;
}

/**
 * Get the blob URL for an attachment path (if tracked).
 */
export function getBlobUrl(originalPath: string): string | undefined {
  return blobUrlMap.get(originalPath);
}

/**
 * Track a blob URL for an attachment path.
 * Use this when creating blob URLs externally (e.g., for file uploads).
 */
export function trackBlobUrl(originalPath: string, blobUrl: string): void {
  blobUrlMap.set(originalPath, blobUrl);
}

/**
 * Check if we have any tracked blob URLs.
 */
export function hasBlobUrls(): boolean {
  return blobUrlMap.size > 0;
}

// ============================================================================
// Path Utilities
// ============================================================================

/**
 * Get the directory portion of a path (browser-compatible dirname).
 */
function getDirectory(filePath: string): string {
  const lastSlash = filePath.lastIndexOf('/');
  return lastSlash >= 0 ? filePath.substring(0, lastSlash) : '';
}

/**
 * Join path segments (browser-compatible path.join).
 */
function joinPaths(...segments: string[]): string {
  return segments
    .filter(s => s.length > 0)
    .join('/')
    .replace(/\/+/g, '/'); // Remove duplicate slashes
}

/**
 * Compute a relative path from one directory to another (browser-compatible).
 */
function relativePath(fromDir: string, toDir: string): string {
  if (fromDir === toDir) return '';

  const fromParts = fromDir.split('/').filter(p => p.length > 0);
  const toParts = toDir.split('/').filter(p => p.length > 0);

  // Find common prefix
  let commonLength = 0;
  while (
    commonLength < fromParts.length &&
    commonLength < toParts.length &&
    fromParts[commonLength] === toParts[commonLength]
  ) {
    commonLength++;
  }

  // Build relative path: go up from 'from', then down to 'to'
  const upCount = fromParts.length - commonLength;
  const ups = Array(upCount).fill('..');
  const downs = toParts.slice(commonLength);

  return [...ups, ...downs].join('/');
}

/**
 * Compute the relative path from the current entry to an attachment
 * that may be defined in an ancestor entry.
 *
 * @param currentEntryPath - Path to the current entry (e.g., "2025/01/day.md")
 * @param sourceEntryPath - Path to entry containing the attachment (e.g., "2025/01.index.md")
 * @param attachmentPath - The attachment path relative to source (e.g., "header.png")
 * @returns Relative path from current entry to attachment
 */
export function computeRelativeAttachmentPath(
  currentEntryPath: string,
  sourceEntryPath: string,
  attachmentPath: string
): string {
  // If same entry, just return attachment path
  if (currentEntryPath === sourceEntryPath) {
    return attachmentPath;
  }

  // Get directories
  const currentDir = getDirectory(currentEntryPath);
  const sourceDir = getDirectory(sourceEntryPath);

  // Compute relative path from current dir to source dir
  const relToSource = relativePath(currentDir, sourceDir);

  // Join with attachment path
  return relToSource ? joinPaths(relToSource, attachmentPath) : attachmentPath;
}
