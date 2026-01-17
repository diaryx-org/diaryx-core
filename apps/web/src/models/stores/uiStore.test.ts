import { describe, it, expect, beforeEach } from 'vitest'
import { getUIStore } from './uiStore.svelte'

describe('uiStore', () => {
  let store: ReturnType<typeof getUIStore>

  beforeEach(() => {
    store = getUIStore()
    // Reset state
    store.setLeftSidebarCollapsed(true)
    store.setRightSidebarCollapsed(true)
    store.closeCommandPalette()
    store.closeSettingsDialog()
    store.closeExportDialog()
    store.closeNewEntryModal()
    store.setError(null)
    store.setExportPath('')
  })

  describe('sidebar states', () => {
    it('should initialize with collapsed sidebars', () => {
      expect(store.leftSidebarCollapsed).toBe(true)
      expect(store.rightSidebarCollapsed).toBe(true)
    })

    it('should toggle left sidebar', () => {
      store.toggleLeftSidebar()
      expect(store.leftSidebarCollapsed).toBe(false)

      store.toggleLeftSidebar()
      expect(store.leftSidebarCollapsed).toBe(true)
    })

    it('should toggle right sidebar', () => {
      store.toggleRightSidebar()
      expect(store.rightSidebarCollapsed).toBe(false)

      store.toggleRightSidebar()
      expect(store.rightSidebarCollapsed).toBe(true)
    })

    it('should set left sidebar collapsed state', () => {
      store.setLeftSidebarCollapsed(false)
      expect(store.leftSidebarCollapsed).toBe(false)

      store.setLeftSidebarCollapsed(true)
      expect(store.leftSidebarCollapsed).toBe(true)
    })

    it('should set right sidebar collapsed state', () => {
      store.setRightSidebarCollapsed(false)
      expect(store.rightSidebarCollapsed).toBe(false)

      store.setRightSidebarCollapsed(true)
      expect(store.rightSidebarCollapsed).toBe(true)
    })
  })

  describe('command palette', () => {
    it('should initialize with closed command palette', () => {
      expect(store.showCommandPalette).toBe(false)
    })

    it('should open command palette', () => {
      store.openCommandPalette()
      expect(store.showCommandPalette).toBe(true)
    })

    it('should close command palette', () => {
      store.openCommandPalette()
      store.closeCommandPalette()
      expect(store.showCommandPalette).toBe(false)
    })

    it('should toggle command palette', () => {
      store.toggleCommandPalette()
      expect(store.showCommandPalette).toBe(true)

      store.toggleCommandPalette()
      expect(store.showCommandPalette).toBe(false)
    })
  })

  describe('settings dialog', () => {
    it('should initialize with closed settings dialog', () => {
      expect(store.showSettingsDialog).toBe(false)
    })

    it('should open settings dialog', () => {
      store.openSettingsDialog()
      expect(store.showSettingsDialog).toBe(true)
    })

    it('should close settings dialog', () => {
      store.openSettingsDialog()
      store.closeSettingsDialog()
      expect(store.showSettingsDialog).toBe(false)
    })

    it('should set settings dialog visibility', () => {
      store.setShowSettingsDialog(true)
      expect(store.showSettingsDialog).toBe(true)

      store.setShowSettingsDialog(false)
      expect(store.showSettingsDialog).toBe(false)
    })
  })

  describe('export dialog', () => {
    it('should initialize with closed export dialog', () => {
      expect(store.showExportDialog).toBe(false)
    })

    it('should open export dialog without path', () => {
      store.openExportDialog()
      expect(store.showExportDialog).toBe(true)
      expect(store.exportPath).toBe('')
    })

    it('should open export dialog with path', () => {
      store.openExportDialog('workspace/notes')
      expect(store.showExportDialog).toBe(true)
      expect(store.exportPath).toBe('workspace/notes')
    })

    it('should close export dialog', () => {
      store.openExportDialog('test')
      store.closeExportDialog()
      expect(store.showExportDialog).toBe(false)
    })

    it('should set export dialog visibility', () => {
      store.setShowExportDialog(true)
      expect(store.showExportDialog).toBe(true)

      store.setShowExportDialog(false)
      expect(store.showExportDialog).toBe(false)
    })

    it('should set export path', () => {
      store.setExportPath('new/path')
      expect(store.exportPath).toBe('new/path')
    })
  })

  describe('new entry modal', () => {
    it('should initialize with closed new entry modal', () => {
      expect(store.showNewEntryModal).toBe(false)
    })

    it('should open new entry modal', () => {
      store.openNewEntryModal()
      expect(store.showNewEntryModal).toBe(true)
    })

    it('should close new entry modal', () => {
      store.openNewEntryModal()
      store.closeNewEntryModal()
      expect(store.showNewEntryModal).toBe(false)
    })

    it('should set new entry modal visibility', () => {
      store.setShowNewEntryModal(true)
      expect(store.showNewEntryModal).toBe(true)

      store.setShowNewEntryModal(false)
      expect(store.showNewEntryModal).toBe(false)
    })
  })

  describe('error management', () => {
    it('should initialize with no error', () => {
      expect(store.error).toBeNull()
    })

    it('should set error', () => {
      store.setError('Something went wrong')
      expect(store.error).toBe('Something went wrong')
    })

    it('should clear error', () => {
      store.setError('Error')
      store.clearError()
      expect(store.error).toBeNull()
    })

    it('should set error to null', () => {
      store.setError('Error')
      store.setError(null)
      expect(store.error).toBeNull()
    })
  })

  describe('editor reference', () => {
    it('should initialize with null editor reference', () => {
      expect(store.editorRef).toBeNull()
    })

    it('should set editor reference', () => {
      const mockEditor = { getJSON: () => ({}) }
      store.setEditorRef(mockEditor)
      expect(store.editorRef).toStrictEqual(mockEditor)
    })
  })
})
