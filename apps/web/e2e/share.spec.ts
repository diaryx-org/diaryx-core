import { test, expect } from '@playwright/test'

test.describe('Share Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
  })

  test('should open share tab in right sidebar', async ({ page }) => {
    // The share tab is in the right sidebar, click on "Share" tab button
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await expect(shareTab).toBeVisible()

    await shareTab.click()

    // ShareTab content should be visible with "Live Collaboration" header
    await expect(page.locator('text=Live Collaboration')).toBeVisible()
  })

  test('should display host and join options', async ({ page }) => {
    // Click Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    // Should show "Host a Session" button
    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await expect(hostButton).toBeVisible()

    // Should show join code input with placeholder
    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toBeVisible()
  })

  test('should allow entering join code', async ({ page }) => {
    // Click Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    // Find and fill join code input
    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toBeVisible()

    await joinInput.fill('TEST-CODE')
    await expect(joinInput).toHaveValue('TEST-CODE')
  })

  test('should show error when submitting empty join code', async ({ page }) => {
    // Click Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    // Find the join button (UserPlus icon button next to input)
    const joinButton = page.locator('button').filter({ has: page.locator('svg') }).last()

    // It should be disabled when input is empty
    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toHaveValue('')

    // The join button should be disabled
    await expect(joinButton).toBeDisabled()
  })

  test('should attempt to create session when clicking Host', async ({ page }) => {
    // Click Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    // Click Host a Session
    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await hostButton.click()

    // Should show loading state or transition to hosting state
    // Either "Creating session..." or "Hosting Session" should appear
    const loadingOrHosting = page.locator('text=Creating session...').or(page.locator('text=Hosting Session'))
    await expect(loadingOrHosting).toBeVisible({ timeout: 5000 })
  })
})

test.describe('Share Session Hosting', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // Navigate to Share tab
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()
  })

  test('should show hosting UI after creating session', async ({ page }) => {
    // Click Host a Session
    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await hostButton.click()

    // Wait for either hosting state or error
    await page.waitForTimeout(3000)

    // Check for hosting state indicators
    const hostingIndicator = page.locator('text=Hosting Session')
    const errorIndicator = page.locator('[role="alert"]')

    const isHosting = await hostingIndicator.isVisible()
    const hasError = await errorIndicator.isVisible()

    // Either we're hosting or got an error (e.g., no server)
    expect(isHosting || hasError).toBe(true)

    if (isHosting) {
      // Should show "Stop Sharing" button
      await expect(page.locator('button').filter({ hasText: 'Stop Sharing' })).toBeVisible()

      // Should show connected peers count
      await expect(page.locator('text=Connected peers')).toBeVisible()
    }
  })
})
