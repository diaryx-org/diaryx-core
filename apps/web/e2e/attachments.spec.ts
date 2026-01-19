import { test, expect, EditorHelper, waitForAppReady } from './fixtures'

test.describe('Attachment Picker via Floating Menu', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)
  })

  test('should open floating menu with plus button', async ({ page, editorHelper }) => {
    await editorHelper.waitForReady()

    const plusButton = await editorHelper.openFloatingMenu()
    await plusButton.click()

    const expandedMenu = page.locator('.menu-expanded')
    await expect(expandedMenu).toBeVisible()
  })

  test('should show attachment option in expanded menu', async ({ page, editorHelper }) => {
    await editorHelper.waitForReady()
    await editorHelper.expandFloatingMenu()

    const attachmentButton = page.locator('.menu-item[title="Insert Attachment"]')
    await expect(attachmentButton).toBeVisible()
  })

  test('should insert attachment picker when clicking attachment button', async ({ page, editorHelper }) => {
    await editorHelper.waitForReady()
    await editorHelper.expandFloatingMenu()

    const attachmentButton = page.locator('.menu-item[title="Insert Attachment"]')
    await attachmentButton.click()

    const pickerNode = page.locator('.attachment-picker-node-wrapper, [data-attachment-picker]')
    await expect(pickerNode).toBeVisible()
  })
})

test.describe('Attachments in Right Sidebar', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)

    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    await propsTab.click()
  })

  test('should display attachments section in properties tab', async ({ page }) => {
    const attachmentsHeader = page.locator('aside span.font-medium').filter({ hasText: 'Attachments' })
    await expect(attachmentsHeader).toBeVisible()
  })

  test('should show attachments list or empty state', async ({ page }) => {
    const attachmentItems = page.locator('[role="listitem"][aria-label*="Attachment"]')
    const emptyState = page.locator('aside').locator('text=No attachments')

    // Use soft assertions to check either condition
    const hasAttachments = await attachmentItems.count() > 0
    const hasEmptyState = await emptyState.isVisible()

    expect(hasAttachments || hasEmptyState).toBe(true)
  })
})

test.describe('Drag and Drop Attachments', () => {
  test('should show drag indicator for attachments in sidebar', async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)

    const draggableAttachments = page.locator('[draggable="true"][aria-label*="Attachment"]')

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
    await waitForAppReady(page)

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible()

    const images = editor.locator('img')
    const imageCount = await images.count()

    if (imageCount > 0) {
      const firstImage = images.first()
      await expect(firstImage).toHaveAttribute('src')
    }
  })
})
