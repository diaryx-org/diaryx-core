/**
 * Attachment Service
 * 
 * Manages blob URLs for displaying attachments in the editor.
 * Handles transforming file paths to blob URLs and back.
 */

import type { Backend } from '$lib/backend';

// ============================================================================
// State
// ============================================================================

// Blob URL tracking for attachments (originalPath -> blobUrl)
const blobUrlMap = new Map<string, string>();

// ============================================================================
// MIME Type Mapping
// ============================================================================

const mimeTypes: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  pdf: 'application/pdf',
};

function getMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return mimeTypes[ext] || 'application/octet-stream';
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
 * @param backend - Backend instance for reading attachment data
 * @returns Content with attachment paths replaced by blob URLs
 */
export async function transformAttachmentPaths(
  content: string,
  entryPath: string,
  backend: Backend | null,
): Promise<string> {
  if (!backend) return content;

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
      const data = await backend.getAttachmentData(entryPath, imagePath);

      // Create blob and URL
      const mimeType = getMimeType(imagePath);
      const blob = new Blob([new Uint8Array(data)], { type: mimeType });
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
