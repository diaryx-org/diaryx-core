import { test, expect } from '@playwright/test'

test.describe('Workspace', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
  })

  test('should load the main page', async ({ page }) => {
    // Verify the app loaded
    await expect(page.locator('body')).toBeVisible()
    const bodyContent = await page.locator('body').textContent()
    expect(bodyContent).toBeTruthy()
  })

  test('should display the editor', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })
  })

  test('should display the right sidebar with tabs', async ({ page }) => {
    // Look for the tab buttons in the right sidebar
    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    const historyTab = page.locator('button').filter({ hasText: 'History' })
    const shareTab = page.locator('button').filter({ hasText: 'Share' })

    await expect(propsTab).toBeVisible()
    await expect(historyTab).toBeVisible()
    await expect(shareTab).toBeVisible()
  })

  test('should switch between sidebar tabs', async ({ page }) => {
    // Click on History tab
    const historyTab = page.locator('button').filter({ hasText: 'History' })
    await historyTab.click()

    // Should show history content
    await expect(page.locator('text=Version History')).toBeVisible()

    // Click on Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    // Should show share content
    await expect(page.locator('text=Live Collaboration')).toBeVisible()

    // Click on Props tab
    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    await propsTab.click()

    // Should show properties or "No properties" message
    const hasProperties = await page.locator('text=Add Property').isVisible()
    expect(hasProperties).toBe(true)
  })

  test('should collapse right sidebar when clicking collapse button', async ({ page }) => {
    // The right sidebar has border-l (border on left side), distinguishing it from the left sidebar (border-r)
    // It also contains the collapse button with aria-label="Collapse panel"
    const collapseButton = page.locator('button[aria-label="Collapse panel"]')
    await expect(collapseButton).toBeVisible()

    // Get the right sidebar (the aside element containing the collapse button)
    const sidebar = page.locator('aside.border-l')
    await expect(sidebar).toBeVisible()

    // Get initial sidebar width
    const initialWidth = await sidebar.evaluate(el => el.getBoundingClientRect().width)
    expect(initialWidth).toBeGreaterThan(0)

    // Click collapse button
    await collapseButton.click()

    await page.waitForTimeout(400) // Wait for animation

    // Sidebar should be collapsed - check for w-0 class or zero/near-zero width
    const hasCollapsedClass = await sidebar.evaluate(el => el.classList.contains('w-0'))
    const collapsedWidth = await sidebar.evaluate(el => el.getBoundingClientRect().width)

    // Either the class indicates collapse OR the width is significantly smaller
    expect(hasCollapsedClass || collapsedWidth < initialWidth / 2).toBe(true)
  })
})

test.describe('Workspace Entry Properties', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // Ensure we're on the Props tab
    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    await propsTab.click()
  })

  test('should show attachments section in properties', async ({ page }) => {
    // The attachments section has a span with "Attachments" text
    const attachmentsHeader = page.locator('aside span.font-medium').filter({ hasText: 'Attachments' })
    await expect(attachmentsHeader).toBeVisible({ timeout: 5000 })
  })

  test('should show Add Property button', async ({ page }) => {
    const addPropertyButton = page.locator('aside').locator('button').filter({ hasText: 'Add Property' })
    await expect(addPropertyButton).toBeVisible()
  })

  test('should open add property form when clicking Add Property', async ({ page }) => {
    const addPropertyButton = page.locator('aside').locator('button').filter({ hasText: 'Add Property' })
    await addPropertyButton.click()

    // Should show property name and value inputs
    const propertyNameInput = page.locator('input[placeholder="Property name..."]')
    const propertyValueInput = page.locator('input[placeholder="Value..."]')

    await expect(propertyNameInput).toBeVisible()
    await expect(propertyValueInput).toBeVisible()
  })
})

test.describe('Workspace Navigation', () => {
  test('should allow keyboard navigation in editor', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible({ timeout: 10000 })

    // Focus editor and type
    await editor.click()
    await page.keyboard.type('Line 1')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Line 2')

    // Verify content
    await expect(editor).toContainText('Line 1')
    await expect(editor).toContainText('Line 2')

    // Navigate with arrow keys
    await page.keyboard.press('ArrowUp')
    await page.keyboard.press('End')
    await page.keyboard.type(' more')

    await expect(editor).toContainText('Line 1 more')
  })
})
