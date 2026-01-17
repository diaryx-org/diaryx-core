import { describe, it, expect, vi, beforeEach } from 'vitest'
import { toast } from 'svelte-sonner'
import {
  showError,
  showSuccess,
  showWarning,
  showInfo,
  showLoading,
  handleError,
} from './toastService'

describe('toastService', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('showError', () => {
    it('should show error toast with string message', () => {
      showError('Something went wrong')

      expect(toast.error).toHaveBeenCalledWith('Something went wrong', {
        description: undefined,
        duration: 5000,
      })
    })

    it('should show error toast with Error object', () => {
      const error = new Error('Test error message')
      showError(error)

      expect(toast.error).toHaveBeenCalledWith('Test error message', {
        description: undefined,
        duration: 5000,
      })
    })

    it('should show error toast with context', () => {
      showError('Failed to save', 'EntryStore')

      expect(toast.error).toHaveBeenCalledWith('Failed to save', {
        description: 'EntryStore',
        duration: 5000,
      })
    })

    it('should log error to console', () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

      showError('Test error')
      expect(consoleSpy).toHaveBeenCalledWith('Test error')

      showError('Test error', 'TestContext')
      expect(consoleSpy).toHaveBeenCalledWith('[TestContext]', 'Test error')

      consoleSpy.mockRestore()
    })
  })

  describe('showSuccess', () => {
    it('should show success toast', () => {
      showSuccess('Operation completed')

      expect(toast.success).toHaveBeenCalledWith('Operation completed', {
        description: undefined,
        duration: 3000,
      })
    })

    it('should show success toast with description', () => {
      showSuccess('Entry saved', 'Changes have been persisted')

      expect(toast.success).toHaveBeenCalledWith('Entry saved', {
        description: 'Changes have been persisted',
        duration: 3000,
      })
    })
  })

  describe('showWarning', () => {
    it('should show warning toast', () => {
      showWarning('Connection unstable')

      expect(toast.warning).toHaveBeenCalledWith('Connection unstable', {
        description: undefined,
        duration: 4000,
      })
    })

    it('should show warning toast with description', () => {
      showWarning('Large file', 'This may take a while to upload')

      expect(toast.warning).toHaveBeenCalledWith('Large file', {
        description: 'This may take a while to upload',
        duration: 4000,
      })
    })
  })

  describe('showInfo', () => {
    it('should show info toast', () => {
      showInfo('New update available')

      expect(toast.info).toHaveBeenCalledWith('New update available', {
        description: undefined,
        duration: 3000,
      })
    })

    it('should show info toast with description', () => {
      showInfo('Tip', 'Press Ctrl+S to save')

      expect(toast.info).toHaveBeenCalledWith('Tip', {
        description: 'Press Ctrl+S to save',
        duration: 3000,
      })
    })
  })

  describe('showLoading', () => {
    it('should show loading toast and return controller', () => {
      vi.mocked(toast.loading).mockReturnValue('toast-123')

      const controller = showLoading('Uploading...')

      expect(toast.loading).toHaveBeenCalledWith('Uploading...')
      expect(controller).toBeDefined()
      expect(typeof controller.success).toBe('function')
      expect(typeof controller.error).toBe('function')
      expect(typeof controller.dismiss).toBe('function')
      expect(typeof controller.update).toBe('function')
    })

    it('should update loading toast to success', () => {
      vi.mocked(toast.loading).mockReturnValue('toast-123')

      const controller = showLoading('Uploading...')
      controller.success('Upload complete!')

      expect(toast.success).toHaveBeenCalledWith('Upload complete!', {
        id: 'toast-123',
        duration: 3000,
      })
    })

    it('should update loading toast to error', () => {
      vi.mocked(toast.loading).mockReturnValue('toast-123')

      const controller = showLoading('Uploading...')
      controller.error('Upload failed!')

      expect(toast.error).toHaveBeenCalledWith('Upload failed!', {
        id: 'toast-123',
        duration: 5000,
      })
    })

    it('should dismiss loading toast', () => {
      vi.mocked(toast.loading).mockReturnValue('toast-123')

      const controller = showLoading('Uploading...')
      controller.dismiss()

      expect(toast.dismiss).toHaveBeenCalledWith('toast-123')
    })

    it('should update loading message', () => {
      vi.mocked(toast.loading).mockReturnValue('toast-123')

      const controller = showLoading('Uploading...')
      controller.update('Processing file...')

      expect(toast.loading).toHaveBeenCalledWith('Processing file...', { id: 'toast-123' })
    })
  })

  describe('handleError', () => {
    it('should handle Error objects', () => {
      const error = new Error('Network failure')
      handleError(error, 'API')

      expect(toast.error).toHaveBeenCalledWith('Network failure', {
        description: 'API',
        duration: 5000,
      })
    })

    it('should handle string errors', () => {
      handleError('Something went wrong', 'Backend')

      expect(toast.error).toHaveBeenCalledWith('Something went wrong', {
        description: 'Backend',
        duration: 5000,
      })
    })

    it('should handle unknown error types', () => {
      handleError({ code: 500 }, 'Server')

      expect(toast.error).toHaveBeenCalledWith('[object Object]', {
        description: 'Server',
        duration: 5000,
      })
    })

    it('should handle null/undefined errors', () => {
      handleError(null, 'Test')
      expect(toast.error).toHaveBeenCalledWith('null', {
        description: 'Test',
        duration: 5000,
      })

      handleError(undefined, 'Test')
      expect(toast.error).toHaveBeenCalledWith('undefined', {
        description: 'Test',
        duration: 5000,
      })
    })
  })
})
