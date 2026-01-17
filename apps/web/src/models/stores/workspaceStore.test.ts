import { describe, it, expect, beforeEach } from 'vitest'
import { getWorkspaceStore } from './workspaceStore.svelte'

describe('workspaceStore', () => {
  let store: ReturnType<typeof getWorkspaceStore>

  beforeEach(() => {
    // Get a fresh store reference
    store = getWorkspaceStore()
    // Reset state
    store.setTree(null)
    store.setExpandedNodes(new Set())
    store.setValidationResult(null)
    store.setWorkspaceCrdtInitialized(false)
    store.setWorkspaceId(null)
    store.setBackend(null)
    store.clearSavedTreeState()
  })

  describe('tree management', () => {
    it('should initialize with null tree', () => {
      expect(store.tree).toBeNull()
    })

    it('should set tree', () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        title: 'Workspace',
        is_index: true,
        children: [],
      }

      store.setTree(mockTree as any)
      expect(store.tree).toEqual(mockTree)
    })

    it('should update subtree', () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        title: 'Workspace',
        is_index: true,
        children: [{
          path: 'workspace/child',
          name: 'child',
          title: 'Child',
          is_index: false,
          children: [],
        }],
      }

      store.setTree(mockTree as any)

      const newSubtree = {
        path: 'workspace/child',
        name: 'child',
        title: 'Child',
        is_index: false,
        children: [{
          path: 'workspace/child/grandchild',
          name: 'grandchild',
          title: 'Grandchild',
          is_index: false,
          children: [],
        }],
      }

      store.updateSubtree('workspace/child', newSubtree as any)

      expect(store.tree?.children[0].children).toHaveLength(1)
      expect(store.tree?.children[0].children[0].name).toBe('grandchild')
    })
  })

  describe('node expansion', () => {
    it('should initialize with empty expanded nodes', () => {
      expect(store.expandedNodes.size).toBe(0)
    })

    it('should toggle node expansion', () => {
      store.toggleNode('path/to/node')
      expect(store.expandedNodes.has('path/to/node')).toBe(true)

      store.toggleNode('path/to/node')
      expect(store.expandedNodes.has('path/to/node')).toBe(false)
    })

    it('should expand node', () => {
      store.expandNode('path/to/node')
      expect(store.expandedNodes.has('path/to/node')).toBe(true)

      // Expanding again should not change state
      store.expandNode('path/to/node')
      expect(store.expandedNodes.has('path/to/node')).toBe(true)
    })

    it('should collapse node', () => {
      store.expandNode('path/to/node')
      store.collapseNode('path/to/node')
      expect(store.expandedNodes.has('path/to/node')).toBe(false)
    })

    it('should set expanded nodes', () => {
      const nodes = new Set(['a', 'b', 'c'])
      store.setExpandedNodes(nodes)
      expect(store.expandedNodes).toEqual(nodes)
    })
  })

  describe('validation', () => {
    it('should initialize with null validation result', () => {
      expect(store.validationResult).toBeNull()
    })

    it('should set validation result', () => {
      const mockResult = {
        errors: [],
        warnings: [],
        scanned_files: 10,
      }

      store.setValidationResult(mockResult as any)
      expect(store.validationResult).toEqual(mockResult)
    })
  })

  describe('workspace CRDT', () => {
    it('should initialize with CRDT not initialized', () => {
      expect(store.workspaceCrdtInitialized).toBe(false)
    })

    it('should set CRDT initialized state', () => {
      store.setWorkspaceCrdtInitialized(true)
      expect(store.workspaceCrdtInitialized).toBe(true)
    })

    it('should set workspace ID', () => {
      store.setWorkspaceId('workspace-123')
      expect(store.workspaceId).toBe('workspace-123')

      store.setWorkspaceId(null)
      expect(store.workspaceId).toBeNull()
    })
  })

  describe('backend', () => {
    it('should initialize with null backend', () => {
      expect(store.backend).toBeNull()
    })

    it('should set backend', () => {
      const mockBackend = { execute: () => {} }
      store.setBackend(mockBackend as any)
      expect(store.backend).toStrictEqual(mockBackend)
    })
  })

  describe('display settings', () => {
    it('should toggle unlinked files visibility', () => {
      store.setShowUnlinkedFiles(true)
      expect(store.showUnlinkedFiles).toBe(true)

      store.setShowUnlinkedFiles(false)
      expect(store.showUnlinkedFiles).toBe(false)
    })

    it('should toggle hidden files visibility', () => {
      store.setShowHiddenFiles(true)
      expect(store.showHiddenFiles).toBe(true)

      store.setShowHiddenFiles(false)
      expect(store.showHiddenFiles).toBe(false)
    })

    it('should toggle editor title visibility', () => {
      store.setShowEditorTitle(true)
      expect(store.showEditorTitle).toBe(true)

      store.setShowEditorTitle(false)
      expect(store.showEditorTitle).toBe(false)
    })

    it('should toggle editor path visibility', () => {
      store.setShowEditorPath(true)
      expect(store.showEditorPath).toBe(true)

      store.setShowEditorPath(false)
      expect(store.showEditorPath).toBe(false)
    })

    it('should toggle readable line length', () => {
      store.setReadableLineLength(true)
      expect(store.readableLineLength).toBe(true)

      store.setReadableLineLength(false)
      expect(store.readableLineLength).toBe(false)
    })

    it('should toggle focus mode', () => {
      store.setFocusMode(true)
      expect(store.focusMode).toBe(true)

      store.setFocusMode(false)
      expect(store.focusMode).toBe(false)
    })

    it('should persist display settings to localStorage', () => {
      store.setShowUnlinkedFiles(true)
      store.setShowHiddenFiles(true)
      store.persistDisplaySettings()

      expect(localStorage.setItem).toHaveBeenCalledWith('diaryx-show-unlinked-files', 'true')
      expect(localStorage.setItem).toHaveBeenCalledWith('diaryx-show-hidden-files', 'true')
    })
  })

  describe('session state management', () => {
    it('should save tree state', () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        title: 'Workspace',
        is_index: true,
        children: [],
      }

      store.setTree(mockTree as any)
      store.expandNode('workspace')

      store.saveTreeState()
      expect(store.hasSavedTreeState()).toBe(true)
    })

    it('should restore tree state', () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        title: 'Workspace',
        is_index: true,
        children: [],
      }

      store.setTree(mockTree as any)
      store.expandNode('workspace')
      store.saveTreeState()

      // Modify current state
      store.setTree(null)
      store.collapseNode('workspace')

      // Restore
      const restored = store.restoreTreeState()
      expect(restored).toBe(true)
      expect(store.tree).toEqual(mockTree)
      expect(store.expandedNodes.has('workspace')).toBe(true)
    })

    it('should return false when no saved state', () => {
      const restored = store.restoreTreeState()
      expect(restored).toBe(false)
    })

    it('should clear saved state without restoring', () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        title: 'Workspace',
        is_index: true,
        children: [],
      }

      store.setTree(mockTree as any)
      store.saveTreeState()
      expect(store.hasSavedTreeState()).toBe(true)

      store.clearSavedTreeState()
      expect(store.hasSavedTreeState()).toBe(false)
    })
  })
})
