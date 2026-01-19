import { test, expect, EditorHelper, waitForAppReady } from './fixtures'

test.describe('Editor', () => {
  let editor: EditorHelper

  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)
    editor = new EditorHelper(page)
    await editor.waitForReady()
  })

  test('should display editor when entry is loaded', async () => {
    await expect(editor.editor).toBeVisible()
  })

  test('should allow typing in the editor', async ({ page }) => {
    await editor.focus()
    await editor.type('Test content')
    await expect(editor.editor).toContainText('Test content')
  })

  test('should apply bold formatting with keyboard shortcut', async ({ page }) => {
    await editor.focus()
    await editor.type('Bold text')
    await editor.selectAll()
    await editor.applyBold()

    // Wait for formatting to apply by checking for the styled element
    const hasBold = await editor.editor.evaluate((el) => {
      const strong = el.querySelector('strong')
      if (strong) return true
      const p = el.querySelector('p')
      if (p) {
        const weight = window.getComputedStyle(p).fontWeight
        return parseInt(weight) >= 600
      }
      return false
    })
    expect(hasBold).toBe(true)
  })

  test('should apply italic formatting with keyboard shortcut', async ({ page }) => {
    await editor.focus()
    await editor.type('Italic text')
    await editor.selectAll()
    await editor.applyItalic()

    const hasItalic = await editor.editor.evaluate((el) => {
      const em = el.querySelector('em')
      if (em) return true
      const p = el.querySelector('p')
      if (p) {
        const style = window.getComputedStyle(p).fontStyle
        return style === 'italic'
      }
      return false
    })
    expect(hasItalic).toBe(true)
  })

  test('should create a heading via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    // Select H1 from heading dropdown
    const headingSelect = page.locator('.heading-section select, [data-slot="native-select"]').first()
    await headingSelect.selectOption('1')

    await page.keyboard.type('My Heading')

    const heading = editor.editor.locator('h1')
    await expect(heading).toContainText('My Heading')
  })

  test('should create a bulleted list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('bullet')

    await page.keyboard.type('Item 1')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Item 2')

    const list = editor.editor.locator('ul')
    await expect(list).toBeVisible()
    const items = editor.editor.locator('li')
    await expect(items).toHaveCount(2)
  })

  test('should create a numbered list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('ordered')

    await page.keyboard.type('First')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Second')

    const orderedList = editor.editor.locator('ol')
    await expect(orderedList).toBeVisible()
  })

  test('should create a task list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('task')

    await page.keyboard.type('My task')

    const taskList = editor.editor.locator('ul[data-type="taskList"]')
    await expect(taskList).toBeVisible()
  })

  test('should create a code block via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const codeBlockButton = page.locator('.menu-item[title="Code Block"]')
    await codeBlockButton.click()

    await page.keyboard.type('const x = 1;')

    const codeBlock = editor.editor.locator('pre')
    await expect(codeBlock).toBeVisible()
    await expect(codeBlock).toContainText('const x = 1')
  })

  test('should create a blockquote via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const quoteButton = page.locator('.menu-item[title="Quote"]')
    await quoteButton.click()

    await page.keyboard.type('A famous quote')

    const blockquote = editor.editor.locator('blockquote')
    await expect(blockquote).toBeVisible()
    await expect(blockquote).toContainText('A famous quote')
  })
})

test.describe('Editor Keyboard Navigation', () => {
  test('should support undo', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const initialLength = (await editorHelper.editor.textContent())?.length || 0

    await editorHelper.focus()
    await editorHelper.type('Hello')
    await expect(editorHelper.editor).toContainText('Hello')

    const afterTypingLength = (await editorHelper.editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    await editorHelper.undo()

    // Wait for undo to take effect
    await expect(async () => {
      const afterUndoLength = (await editorHelper.editor.textContent())?.length || 0
      expect(afterUndoLength).toBeLessThan(afterTypingLength)
    }).toPass({ timeout: 3000 })
  })

  test('should support redo', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const initialLength = (await editorHelper.editor.textContent())?.length || 0

    await editorHelper.focus()
    await editorHelper.type('Test')

    const afterTypingLength = (await editorHelper.editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    await editorHelper.undo()

    // Wait for undo
    await expect(async () => {
      const len = (await editorHelper.editor.textContent())?.length || 0
      expect(len).toBeLessThan(afterTypingLength)
    }).toPass({ timeout: 3000 })

    await editorHelper.redo()

    // Wait for redo
    await expect(async () => {
      const afterRedoLength = (await editorHelper.editor.textContent())?.length || 0
      expect(afterRedoLength).toBe(afterTypingLength)
    }).toPass({ timeout: 3000 })
  })
})

test.describe('Editor Floating Menu', () => {
  test('should show floating menu on empty line', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    await editorHelper.focus()
    await editorHelper.type('temp text')
    await editorHelper.clearContent()

    const floatingMenu = page.locator('.floating-menu')
    await expect(floatingMenu).toBeVisible()
  })

  test('should expand menu when clicking plus button', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const plusButton = await editorHelper.openFloatingMenu()
    await plusButton.click()

    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()
  })

  test('should close expanded menu with Escape', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    await editorHelper.expandFloatingMenu()

    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()

    await page.keyboard.press('Escape')

    await expect(expandedMenu).not.toBeVisible()
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible()
  })
})
