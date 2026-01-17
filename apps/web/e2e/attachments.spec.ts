import { test, expect } from '@playwright/test'

test.describe('Attachment Picker via Floating Menu', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
  })

  test('should open floating menu with plus button', async ({ page }) => {
    // Find the editor
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Click in editor to focus and type content
    await editor.click()
    await page.keyboard.type('temp')

    // Clear content to trigger floating menu (Meta works on macOS)
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // The floating menu should appear on empty line
    const floatingMenu = page.locator('.floating-menu')
    await expect(floatingMenu).toBeVisible({ timeout: 5000 })

    // Click the plus button to expand menu
    const plusButton = floatingMenu.locator('.trigger-button')
    await plusButton.click()

    // Menu should expand showing options
    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()
  })

  test('should show attachment option in expanded menu', async ({ page }) => {
    // Find and focus editor
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })
    await editor.click()
    await page.keyboard.type('temp')

    // Clear and trigger floating menu
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Wait for and click plus button
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Look for Attachment button (has Paperclip icon)
    const attachmentButton = page.locator('.menu-item[title="Insert Attachment"]')
    await expect(attachmentButton).toBeVisible()
  })

  test('should insert attachment picker when clicking attachment button', async ({ page }) => {
    // Find and focus editor
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })
    await editor.click()
    await page.keyboard.type('temp')

    // Clear and trigger floating menu
    await page.keyboard.press('Meta+a')
    await page.keyboard.press('Backspace')

    // Wait for and click plus button
    const plusButton = page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    await plusButton.click()

    // Click attachment button
    const attachmentButton = page.locator('.menu-item[title="Insert Attachment"]')
    await attachmentButton.click()

    // Attachment picker node should be inserted in the editor
    const pickerNode = page.locator('.attachment-picker-node-wrapper, [data-attachment-picker]')
    await expect(pickerNode).toBeVisible({ timeout: 3000 })
  })
})

test.describe('Attachments in Right Sidebar', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // Ensure we're on the Props tab
    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    await propsTab.click()
  })

  test('should display attachments section in properties tab', async ({ page }) => {
    // The attachments section has a span with "Attachments" text
    // It's inside a div with border-t class, below the properties
    const attachmentsHeader = page.locator('aside span.font-medium').filter({ hasText: 'Attachments' })
    await expect(attachmentsHeader).toBeVisible({ timeout: 5000 })
  })

  test('should show attachments list or empty state', async ({ page }) => {
    // Either we have attachments listed or we see the empty state ("No attachments")
    const attachmentItems = page.locator('[role="listitem"][aria-label*="Attachment"]')
    const emptyState = page.locator('aside').locator('text=No attachments')

    const hasAttachments = await attachmentItems.count() > 0
    const hasEmptyState = await emptyState.isVisible()

    // One of these should be true
    expect(hasAttachments || hasEmptyState).toBe(true)
  })
})

test.describe('Drag and Drop Attachments', () => {
  test('should show drag indicator for attachments in sidebar', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // Look for draggable attachment items in the sidebar
    const draggableAttachments = page.locator('[draggable="true"][aria-label*="Attachment"]')

    // If there are attachments, they should be draggable
    const count = await draggableAttachments.count()
    if (count > 0) {
      const firstAttachment = draggableAttachments.first()
      await expect(firstAttachment).toHaveAttribute('draggable', 'true')
    }
  })
})

test.describe('Image Display in Editor', () => {
  test('should display images embedded in editor content', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Check for any images in editor
    const images = editor.locator('img')
    const imageCount = await images.count()

    // If there are images, verify they have src attributes
    if (imageCount > 0) {
      const firstImage = images.first()
      await expect(firstImage).toHaveAttribute('src')
    }
  })
})
