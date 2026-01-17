import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { getEntryStore } from './entryStore.svelte'

describe('entryStore', () => {
  let store: ReturnType<typeof getEntryStore>

  beforeEach(() => {
    vi.useFakeTimers()
    store = getEntryStore()
    // Reset state
    store.setEntry(null)
    store.setDisplayContent('')
    store.markClean()
    store.setSaving(false)
    store.setLoading(true)
    store.setTitleError(null)
    store.cancelAutoSave()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('entry management', () => {
    it('should initialize with null entry', () => {
      expect(store.currentEntry).toBeNull()
    })

    it('should set entry and reset dirty state', () => {
      store.markDirty()
      expect(store.isDirty).toBe(true)

      const mockEntry = {
        path: 'test.md',
        title: 'Test',
        content: '# Test',
        frontmatter: { title: 'Test' },
      }

      store.setEntry(mockEntry as any)
      expect(store.currentEntry).toEqual(mockEntry)
      expect(store.isDirty).toBe(false)
      expect(store.titleError).toBeNull()
    })

    it('should set current entry without resetting dirty state', () => {
      store.markDirty()

      const mockEntry = {
        path: 'test.md',
        title: 'Test',
        content: '# Test',
        frontmatter: { title: 'Test' },
      }

      store.setCurrentEntry(mockEntry as any)
      expect(store.currentEntry).toEqual(mockEntry)
      expect(store.isDirty).toBe(true)
    })
  })

  describe('display content', () => {
    it('should initialize with empty display content', () => {
      expect(store.displayContent).toBe('')
    })

    it('should set display content', () => {
      store.setDisplayContent('# Hello World')
      expect(store.displayContent).toBe('# Hello World')
    })
  })

  describe('dirty state', () => {
    it('should initialize as not dirty', () => {
      expect(store.isDirty).toBe(false)
    })

    it('should mark as dirty', () => {
      store.markDirty()
      expect(store.isDirty).toBe(true)
    })

    it('should mark as clean', () => {
      store.markDirty()
      store.markClean()
      expect(store.isDirty).toBe(false)
    })
  })

  describe('saving state', () => {
    it('should initialize as not saving', () => {
      expect(store.isSaving).toBe(false)
    })

    it('should set saving state', () => {
      store.setSaving(true)
      expect(store.isSaving).toBe(true)

      store.setSaving(false)
      expect(store.isSaving).toBe(false)
    })
  })

  describe('loading state', () => {
    it('should initialize as loading', () => {
      expect(store.isLoading).toBe(true)
    })

    it('should set loading state', () => {
      store.setLoading(false)
      expect(store.isLoading).toBe(false)

      store.setLoading(true)
      expect(store.isLoading).toBe(true)
    })
  })

  describe('title error', () => {
    it('should initialize with no title error', () => {
      expect(store.titleError).toBeNull()
    })

    it('should set title error', () => {
      store.setTitleError('Title cannot be empty')
      expect(store.titleError).toBe('Title cannot be empty')
    })

    it('should clear title error', () => {
      store.setTitleError('Error')
      store.setTitleError(null)
      expect(store.titleError).toBeNull()
    })
  })

  describe('auto-save', () => {
    it('should schedule auto-save callback', () => {
      const saveCallback = vi.fn()
      store.markDirty()
      store.scheduleAutoSave(saveCallback)

      expect(saveCallback).not.toHaveBeenCalled()

      // Fast-forward past the auto-save delay (2500ms)
      vi.advanceTimersByTime(2500)

      expect(saveCallback).toHaveBeenCalledTimes(1)
    })

    it('should not call save callback if not dirty', () => {
      const saveCallback = vi.fn()
      store.markClean()
      store.scheduleAutoSave(saveCallback)

      vi.advanceTimersByTime(2500)

      expect(saveCallback).not.toHaveBeenCalled()
    })

    it('should cancel pending auto-save', () => {
      const saveCallback = vi.fn()
      store.markDirty()
      store.scheduleAutoSave(saveCallback)

      // Cancel before the timer fires
      store.cancelAutoSave()

      vi.advanceTimersByTime(5000)

      expect(saveCallback).not.toHaveBeenCalled()
    })

    it('should cancel previous auto-save when scheduling new one', () => {
      const saveCallback1 = vi.fn()
      const saveCallback2 = vi.fn()

      store.markDirty()
      store.scheduleAutoSave(saveCallback1)

      // Schedule new auto-save before first one fires
      vi.advanceTimersByTime(1000)
      store.scheduleAutoSave(saveCallback2)

      // First callback's timer is cancelled
      vi.advanceTimersByTime(1500) // Total 2500ms from first schedule
      expect(saveCallback1).not.toHaveBeenCalled()

      // Second callback fires after its full delay
      vi.advanceTimersByTime(1000) // Total 2500ms from second schedule
      expect(saveCallback2).toHaveBeenCalledTimes(1)
    })
  })
})
