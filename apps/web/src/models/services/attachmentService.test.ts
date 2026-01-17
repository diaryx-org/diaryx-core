import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  getMimeType,
  isHeicFile,
  convertHeicToJpeg,
  revokeBlobUrls,
  transformAttachmentPaths,
  reverseBlobUrlsToAttachmentPaths,
  getBlobUrl,
  trackBlobUrl,
  hasBlobUrls,
  computeRelativeAttachmentPath,
} from './attachmentService'

describe('attachmentService', () => {
  beforeEach(() => {
    // Clean up blob URLs before each test
    revokeBlobUrls()
  })

  describe('getMimeType', () => {
    it('should return correct MIME type for common image extensions', () => {
      expect(getMimeType('photo.png')).toBe('image/png')
      expect(getMimeType('photo.jpg')).toBe('image/jpeg')
      expect(getMimeType('photo.jpeg')).toBe('image/jpeg')
      expect(getMimeType('photo.gif')).toBe('image/gif')
      expect(getMimeType('photo.webp')).toBe('image/webp')
      expect(getMimeType('photo.svg')).toBe('image/svg+xml')
      expect(getMimeType('photo.bmp')).toBe('image/bmp')
      expect(getMimeType('favicon.ico')).toBe('image/x-icon')
    })

    it('should return correct MIME type for HEIC/HEIF', () => {
      expect(getMimeType('photo.heic')).toBe('image/heic')
      expect(getMimeType('photo.heif')).toBe('image/heif')
    })

    it('should return correct MIME type for document extensions', () => {
      expect(getMimeType('doc.pdf')).toBe('application/pdf')
      expect(getMimeType('doc.doc')).toBe('application/msword')
      expect(getMimeType('doc.docx')).toBe('application/vnd.openxmlformats-officedocument.wordprocessingml.document')
      expect(getMimeType('doc.xls')).toBe('application/vnd.ms-excel')
      expect(getMimeType('doc.xlsx')).toBe('application/vnd.openxmlformats-officedocument.spreadsheetml.sheet')
      expect(getMimeType('doc.ppt')).toBe('application/vnd.ms-powerpoint')
      expect(getMimeType('doc.pptx')).toBe('application/vnd.openxmlformats-officedocument.presentationml.presentation')
    })

    it('should return correct MIME type for text files', () => {
      expect(getMimeType('file.txt')).toBe('text/plain')
      expect(getMimeType('file.md')).toBe('text/markdown')
      expect(getMimeType('file.csv')).toBe('text/csv')
      expect(getMimeType('file.json')).toBe('application/json')
      expect(getMimeType('file.xml')).toBe('application/xml')
    })

    it('should return correct MIME type for archives', () => {
      expect(getMimeType('archive.zip')).toBe('application/zip')
      expect(getMimeType('archive.tar')).toBe('application/x-tar')
      expect(getMimeType('archive.gz')).toBe('application/gzip')
      expect(getMimeType('archive.7z')).toBe('application/x-7z-compressed')
      expect(getMimeType('archive.rar')).toBe('application/vnd.rar')
    })

    it('should return octet-stream for unknown extensions', () => {
      expect(getMimeType('file.unknown')).toBe('application/octet-stream')
      expect(getMimeType('file.xyz')).toBe('application/octet-stream')
      expect(getMimeType('file')).toBe('application/octet-stream')
    })

    it('should be case-insensitive', () => {
      expect(getMimeType('photo.PNG')).toBe('image/png')
      expect(getMimeType('photo.JPG')).toBe('image/jpeg')
      expect(getMimeType('photo.HEIC')).toBe('image/heic')
    })

    it('should handle paths with directories', () => {
      expect(getMimeType('folder/subfolder/photo.png')).toBe('image/png')
      expect(getMimeType('/absolute/path/doc.pdf')).toBe('application/pdf')
    })
  })

  describe('isHeicFile', () => {
    it('should return true for HEIC files', () => {
      expect(isHeicFile('photo.heic')).toBe(true)
      expect(isHeicFile('photo.HEIC')).toBe(true)
    })

    it('should return true for HEIF files', () => {
      expect(isHeicFile('photo.heif')).toBe(true)
      expect(isHeicFile('photo.HEIF')).toBe(true)
    })

    it('should return false for other file types', () => {
      expect(isHeicFile('photo.jpg')).toBe(false)
      expect(isHeicFile('photo.png')).toBe(false)
      expect(isHeicFile('photo.gif')).toBe(false)
    })

    it('should handle paths with directories', () => {
      expect(isHeicFile('folder/photo.heic')).toBe(true)
      expect(isHeicFile('/path/to/photo.heif')).toBe(true)
    })
  })

  describe('convertHeicToJpeg', () => {
    it('should convert HEIC blob to JPEG', async () => {
      const heicBlob = new Blob(['mock-heic-data'], { type: 'image/heic' })
      const result = await convertHeicToJpeg(heicBlob)

      expect(result).toBeInstanceOf(Blob)
      expect(result.type).toBe('image/jpeg')
    })

    it('should return original blob on conversion failure', async () => {
      // Mock heic2any to throw an error
      const heic2any = await import('heic2any')
      vi.mocked(heic2any.default).mockRejectedValueOnce(new Error('Conversion failed'))

      const heicBlob = new Blob(['mock-heic-data'], { type: 'image/heic' })
      const result = await convertHeicToJpeg(heicBlob)

      expect(result).toBe(heicBlob)
    })
  })

  describe('blob URL tracking', () => {
    it('should track blob URLs', () => {
      expect(hasBlobUrls()).toBe(false)

      trackBlobUrl('image.png', 'blob:mock-url-1')
      expect(hasBlobUrls()).toBe(true)
      expect(getBlobUrl('image.png')).toBe('blob:mock-url-1')
    })

    it('should return undefined for untracked paths', () => {
      expect(getBlobUrl('unknown.png')).toBeUndefined()
    })

    it('should revoke all blob URLs', () => {
      trackBlobUrl('image1.png', 'blob:url-1')
      trackBlobUrl('image2.png', 'blob:url-2')

      expect(hasBlobUrls()).toBe(true)

      revokeBlobUrls()

      expect(hasBlobUrls()).toBe(false)
      expect(getBlobUrl('image1.png')).toBeUndefined()
      expect(getBlobUrl('image2.png')).toBeUndefined()
    })
  })

  describe('transformAttachmentPaths', () => {
    it('should return original content if api is null', async () => {
      const content = '![alt](image.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', null)
      expect(result).toBe(content)
    })

    it('should skip external URLs', async () => {
      const mockApi = {
        getAttachmentData: vi.fn(),
      }

      const content = '![alt](https://example.com/image.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
      expect(mockApi.getAttachmentData).not.toHaveBeenCalled()
    })

    it('should skip already-transformed blob URLs', async () => {
      const mockApi = {
        getAttachmentData: vi.fn(),
      }

      const content = '![alt](blob:http://localhost/123)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
      expect(mockApi.getAttachmentData).not.toHaveBeenCalled()
    })

    it('should transform local image paths to blob URLs', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        getAttachmentData: vi.fn().mockResolvedValue(Array.from(mockData)),
      }

      const content = '![test image](test.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.getAttachmentData).toHaveBeenCalledWith('entry.md', 'test.png')
      expect(result).toContain('blob:')
      expect(result).toContain('![test image]')
    })

    it('should handle angle-bracket syntax for paths with spaces', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        getAttachmentData: vi.fn().mockResolvedValue(Array.from(mockData)),
      }

      const content = '![alt](<path with spaces/image.png>)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.getAttachmentData).toHaveBeenCalledWith('entry.md', 'path with spaces/image.png')
    })

    it('should leave original path on attachment not found', async () => {
      const mockApi = {
        getAttachmentData: vi.fn().mockRejectedValue(new Error('Not found')),
      }

      const content = '![alt](missing.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
    })
  })

  describe('reverseBlobUrlsToAttachmentPaths', () => {
    it('should reverse blob URLs to original paths', () => {
      trackBlobUrl('image.png', 'blob:url-1')

      const content = 'Here is an image: ![alt](blob:url-1)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('Here is an image: ![alt](image.png)')
    })

    it('should wrap paths with spaces in angle brackets', () => {
      trackBlobUrl('path with spaces/image.png', 'blob:url-2')

      const content = '![alt](blob:url-2)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('![alt](<path with spaces/image.png>)')
    })

    it('should handle multiple blob URLs', () => {
      trackBlobUrl('image1.png', 'blob:url-1')
      trackBlobUrl('image2.png', 'blob:url-2')

      const content = '![one](blob:url-1) and ![two](blob:url-2)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('![one](image1.png) and ![two](image2.png)')
    })

    it('should return original content if no blob URLs are tracked', () => {
      const content = '![alt](image.png)'
      const result = reverseBlobUrlsToAttachmentPaths(content)
      expect(result).toBe(content)
    })
  })

  describe('computeRelativeAttachmentPath', () => {
    it('should return attachment path if same entry', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/day.md',
        '2025/01/day.md',
        'image.png'
      )
      expect(result).toBe('image.png')
    })

    it('should compute relative path from child to parent', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/day.md',
        '2025/01.index.md',
        'header.png'
      )
      expect(result).toBe('../header.png')
    })

    it('should compute relative path between sibling directories', () => {
      const result = computeRelativeAttachmentPath(
        '2025/02/day.md',
        '2025/01/note.md',
        'image.png'
      )
      expect(result).toBe('../01/image.png')
    })

    it('should compute relative path from deeply nested to root', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/15/entry.md',
        '2025.index.md',
        'banner.png'
      )
      // 2025/01/15/entry.md -> 2025.index.md needs 3 levels up: ../../../
      expect(result).toBe('../../../banner.png')
    })

    it('should handle root-level entries', () => {
      const result = computeRelativeAttachmentPath(
        'notes.md',
        'index.md',
        'logo.png'
      )
      expect(result).toBe('logo.png')
    })

    it('should handle same directory different files', () => {
      const result = computeRelativeAttachmentPath(
        'docs/a.md',
        'docs/b.md',
        'shared.png'
      )
      expect(result).toBe('shared.png')
    })
  })
})
