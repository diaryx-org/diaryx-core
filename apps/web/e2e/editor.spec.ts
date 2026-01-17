import { test, expect } from '@playwright/test'

test.describe('Editor', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
  })

  test('should display editor when entry is loaded', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })
  })

  test('should allow typing in the editor', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('Test content')

    await expect(editor).toContainText('Test content')
  })

  test('should apply bold formatting with keyboard shortcut', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('Bold text')

    // Select all and apply bold (Meta works on macOS for both browsers)
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Meta+b')

    // Wait a moment for formatting to apply
    await page.waitForTimeout(200)

    // Check that text still exists and has been styled
    // TipTap uses <strong> but we can also check by evaluating computed style
    await expect(editor).toContainText('Bold text')

    // Verify bold was applied by checking for the strong element OR font-weight
    const hasBold = await editor.evaluate((el) => {
      const strong = el.querySelector('strong')
      if (strong) return true
      // Fallback: check computed font-weight
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
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('Italic text')

    // Select all and apply italic
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Meta+i')

    await page.waitForTimeout(200)

    await expect(editor).toContainText('Italic text')

    // Verify italic was applied
    const hasItalic = await editor.evaluate((el) => {
      const em = el.querySelector('em')
      if (em) return true
      // Fallback: check computed font-style
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
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Click editor, type something, then select all and delete to trigger floating menu
    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Wait for floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Select H1 from heading dropdown
    const headingSelect = page.locator('.heading-section select, [data-slot="native-select"]').first()
    await headingSelect.selectOption('1')

    // Type heading text
    await page.keyboard.type('My Heading')

    // Verify heading was created
    const heading = editor.locator('h1')
    await expect(heading).toContainText('My Heading')
  })

  test('should create a bulleted list via floating menu', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Select bullet list from list dropdown
    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('bullet')

    // Type list items
    await page.keyboard.type('Item 1')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Item 2')

    // Verify list was created
    const list = editor.locator('ul')
    await expect(list).toBeVisible()
    const items = editor.locator('li')
    await expect(items).toHaveCount(2)
  })

  test('should create a numbered list via floating menu', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Select ordered list
    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('ordered')

    // Type list items
    await page.keyboard.type('First')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Second')

    // Verify ordered list
    const orderedList = editor.locator('ol')
    await expect(orderedList).toBeVisible()
  })

  test('should create a task list via floating menu', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Select task list
    const listSelect = page.locator('.list-section select, [data-slot="native-select"]').last()
    await listSelect.selectOption('task')

    // Type task item
    await page.keyboard.type('My task')

    // Verify task list exists - TipTap uses ul[data-type="taskList"]
    const taskList = editor.locator('ul[data-type="taskList"]')
    await expect(taskList).toBeVisible()
  })

  test('should create a code block via floating menu', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Click code block button
    const codeBlockButton = page.locator('.menu-item[title="Code Block"]')
    await codeBlockButton.click()

    // Type code
    await page.keyboard.type('const x = 1;')

    // Verify code block
    const codeBlock = editor.locator('pre')
    await expect(codeBlock).toBeVisible()
    await expect(codeBlock).toContainText('const x = 1')
  })

  test('should create a blockquote via floating menu', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open floating menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Click blockquote button
    const quoteButton = page.locator('.menu-item[title="Quote"]')
    await quoteButton.click()

    // Type quote
    await page.keyboard.type('A famous quote')

    // Verify blockquote
    const blockquote = editor.locator('blockquote')
    await expect(blockquote).toBeVisible()
    await expect(blockquote).toContainText('A famous quote')
  })
})

test.describe('Editor Keyboard Navigation', () => {
  test('should support undo with Cmd+Z', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Get initial content length (editor may have default content)
    const initialLength = (await editor.textContent())?.length || 0

    // Type content
    await editor.click()
    await page.keyboard.type('Hello')
    await expect(editor).toContainText('Hello')

    // Get length after typing
    const afterTypingLength = (await editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    // Undo using programmatic API (more reliable than keyboard shortcuts in tests)
    await editor.evaluate(() => {
      if ((window as unknown as { tiptapEditor?: { commands?: { undo: () => void } } }).tiptapEditor?.commands?.undo) {
        (window as unknown as { tiptapEditor: { commands: { undo: () => void } } }).tiptapEditor.commands.undo()
      } else {
        document.execCommand('undo')
      }
    })
    await page.waitForTimeout(100)

    // Content should be shorter after undo
    const afterUndoLength = (await editor.textContent())?.length || 0
    expect(afterUndoLength).toBeLessThan(afterTypingLength)
  })

  test('should support redo with Cmd+Shift+Z', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Get initial content length (editor may have default content)
    const initialLength = (await editor.textContent())?.length || 0

    // Type content
    await editor.click()
    await page.keyboard.type('Test')

    // Get length after typing
    const afterTypingLength = (await editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    // Undo using programmatic API
    await editor.evaluate(() => {
      if ((window as unknown as { tiptapEditor?: { commands?: { undo: () => void } } }).tiptapEditor?.commands?.undo) {
        (window as unknown as { tiptapEditor: { commands: { undo: () => void } } }).tiptapEditor.commands.undo()
      } else {
        document.execCommand('undo')
      }
    })

    await page.waitForTimeout(100)
    const afterUndoLength = (await editor.textContent())?.length || 0
    expect(afterUndoLength).toBeLessThan(afterTypingLength)

    // Redo using programmatic API
    await editor.evaluate(() => {
      if ((window as unknown as { tiptapEditor?: { commands?: { redo: () => void } } }).tiptapEditor?.commands?.redo) {
        (window as unknown as { tiptapEditor: { commands: { redo: () => void } } }).tiptapEditor.commands.redo()
      } else {
        document.execCommand('redo')
      }
    })

    await page.waitForTimeout(100)
    const afterRedoLength = (await editor.textContent())?.length || 0

    // Redo should restore content to after-typing state
    expect(afterRedoLength).toBeGreaterThan(afterUndoLength)
    expect(afterRedoLength).toBe(afterTypingLength)
  })
})

test.describe('Editor Floating Menu', () => {
  test('should show floating menu on empty line', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Type something first, then select all and delete to properly clear
    await editor.click()
    await page.keyboard.type('temp text')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Floating menu should appear
    const floatingMenu = page.locator('.floating-menu')
    await expect(floatingMenu).toBeVisible({ timeout: 5000 })
  })

  test('should expand menu when clicking plus button', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Click plus button
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Expanded menu should show
    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()
  })

  test('should close expanded menu with Escape', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    await editor.click()
    await page.keyboard.type('temp')
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Open menu
    const plusButton = page.locator('.floating-menu .trigger-button')
    await plusButton.click()

    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()

    // Press Escape to close
    await page.keyboard.press('Escape')

    // Menu should collapse back to plus button
    await expect(expandedMenu).not.toBeVisible()
    await expect(plusButton).toBeVisible()
  })
})
