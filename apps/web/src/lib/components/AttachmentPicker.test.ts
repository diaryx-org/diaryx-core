import { describe, it, expect } from 'vitest'
import {
  isImageFile,
  getMimeType,
  getFilename,
  bytesToBase64,
} from '$lib/../models/services/attachmentService'

describe('AttachmentPicker utilities', () => {
  describe('isImageFile', () => {
    it('should identify PNG files as images', () => {
      expect(isImageFile('test.png')).toBe(true)
      expect(isImageFile('folder/image.PNG')).toBe(true)
    })

    it('should identify JPEG files as images', () => {
      expect(isImageFile('photo.jpg')).toBe(true)
      expect(isImageFile('photo.jpeg')).toBe(true)
      expect(isImageFile('photo.JPG')).toBe(true)
    })

    it('should identify GIF files as images', () => {
      expect(isImageFile('animation.gif')).toBe(true)
    })

    it('should identify WebP files as images', () => {
      expect(isImageFile('modern.webp')).toBe(true)
    })

    it('should identify SVG files as images', () => {
      expect(isImageFile('icon.svg')).toBe(true)
    })

    it('should identify HEIC/HEIF files as images', () => {
      expect(isImageFile('apple-photo.heic')).toBe(true)
      expect(isImageFile('apple-photo.heif')).toBe(true)
    })

    it('should not identify non-image files as images', () => {
      expect(isImageFile('document.pdf')).toBe(false)
      expect(isImageFile('data.csv')).toBe(false)
      expect(isImageFile('text.txt')).toBe(false)
      expect(isImageFile('archive.zip')).toBe(false)
    })

    it('should handle files without extensions', () => {
      expect(isImageFile('noextension')).toBe(false)
    })

    it('should handle paths with multiple dots', () => {
      expect(isImageFile('my.image.file.png')).toBe(true)
      expect(isImageFile('my.document.pdf')).toBe(false)
    })
  })

  describe('getMimeType', () => {
    it('should return correct MIME type for images', () => {
      expect(getMimeType('image.png')).toBe('image/png')
      expect(getMimeType('image.jpg')).toBe('image/jpeg')
      expect(getMimeType('image.gif')).toBe('image/gif')
      expect(getMimeType('image.webp')).toBe('image/webp')
      expect(getMimeType('image.svg')).toBe('image/svg+xml')
    })

    it('should return correct MIME type for documents', () => {
      expect(getMimeType('doc.pdf')).toBe('application/pdf')
      expect(getMimeType('doc.doc')).toBe('application/msword')
      expect(getMimeType('doc.xlsx')).toBe('application/vnd.openxmlformats-officedocument.spreadsheetml.sheet')
    })

    it('should return correct MIME type for text files', () => {
      expect(getMimeType('file.txt')).toBe('text/plain')
      expect(getMimeType('file.md')).toBe('text/markdown')
      expect(getMimeType('file.csv')).toBe('text/csv')
      expect(getMimeType('file.json')).toBe('application/json')
    })

    it('should return octet-stream for unknown types', () => {
      expect(getMimeType('file.unknown')).toBe('application/octet-stream')
      expect(getMimeType('noextension')).toBe('application/octet-stream')
    })
  })

  describe('getFilename', () => {
    it('should extract filename from path', () => {
      expect(getFilename('folder/subfolder/file.png')).toBe('file.png')
      expect(getFilename('file.txt')).toBe('file.txt')
    })

    it('should handle deeply nested paths', () => {
      expect(getFilename('a/b/c/d/e/file.md')).toBe('file.md')
    })

    it('should return the path if no slashes', () => {
      expect(getFilename('filename.txt')).toBe('filename.txt')
    })

    it('should handle empty string after last slash', () => {
      // When path ends with slash, pop() returns empty string, so it falls back to path
      expect(getFilename('folder/')).toBe('folder/')
    })
  })

  describe('bytesToBase64', () => {
    it('should convert small byte arrays to base64', () => {
      const bytes = new Uint8Array([72, 101, 108, 108, 111]) // "Hello"
      const result = bytesToBase64(bytes)
      expect(result).toBe(btoa('Hello'))
    })

    it('should handle empty byte array', () => {
      const bytes = new Uint8Array([])
      const result = bytesToBase64(bytes)
      expect(result).toBe('')
    })

    it('should handle binary data', () => {
      const bytes = new Uint8Array([0, 1, 2, 255, 254, 253])
      const result = bytesToBase64(bytes)
      // Verify it's valid base64 by decoding
      const decoded = atob(result)
      expect(decoded.length).toBe(6)
    })

    it('should handle large byte arrays by chunking', () => {
      // Create array larger than chunk size (8192)
      const largeArray = new Uint8Array(10000)
      for (let i = 0; i < largeArray.length; i++) {
        largeArray[i] = i % 256
      }
      const result = bytesToBase64(largeArray)
      // Should not throw and should produce valid base64
      expect(() => atob(result)).not.toThrow()
      expect(atob(result).length).toBe(10000)
    })
  })
})
