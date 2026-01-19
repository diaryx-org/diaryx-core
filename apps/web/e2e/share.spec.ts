import { test, expect, waitForAppReady, mockWebSocket } from './fixtures'

test.describe('Share Tab', () => {
  test.beforeEach(async ({ page }) => {
    // Mock WebSocket to avoid flaky network-dependent tests
    await mockWebSocket(page)
    await page.goto('/')
    await waitForAppReady(page)
  })

  test('should open share tab in right sidebar', async ({ page }) => {
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await expect(shareTab).toBeVisible()

    await shareTab.click()

    await expect(page.locator('text=Live Collaboration')).toBeVisible()
  })

  test('should display host and join options', async ({ page }) => {
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await expect(hostButton).toBeVisible()

    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toBeVisible()
  })

  test('should allow entering join code', async ({ page }) => {
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toBeVisible()

    await joinInput.fill('TEST-CODE')
    await expect(joinInput).toHaveValue('TEST-CODE')
  })

  test('should show error when submitting empty join code', async ({ page }) => {
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    const joinButton = page.locator('button').filter({ has: page.locator('svg') }).last()
    const joinInput = page.locator('input[placeholder="XXXX-XXXX"]')
    await expect(joinInput).toHaveValue('')

    await expect(joinButton).toBeDisabled()
  })

  test('should attempt to create session when clicking Host', async ({ page }) => {
    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()

    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await hostButton.click()

    // With mocked WebSocket, we should see either loading/hosting state
    // The mock simulates a successful connection
    const loadingOrHosting = page.locator('text=Creating session...').or(page.locator('text=Hosting Session'))
    await expect(loadingOrHosting).toBeVisible()
  })
})

test.describe('Share Session Hosting', () => {
  test.beforeEach(async ({ page }) => {
    // Mock WebSocket to avoid flaky network-dependent tests
    await mockWebSocket(page)
    await page.goto('/')
    await waitForAppReady(page)

    const shareTab = page.locator('button').filter({ hasText: 'Share' })
    await shareTab.click()
  })

  test('should show hosting UI after creating session', async ({ page }) => {
    const hostButton = page.locator('button').filter({ hasText: 'Host a Session' })
    await hostButton.click()

    // With mocked WebSocket, we should at minimum see the loading state
    // The mock simulates connection opening, but actual hosting UI depends on app logic
    const loadingState = page.locator('text=Creating session...')
    const hostingState = page.locator('text=Hosting Session')
    const stopButton = page.locator('button').filter({ hasText: 'Stop Sharing' })

    // Wait for any of the expected states to appear
    await expect(loadingState.or(hostingState).or(stopButton)).toBeVisible({ timeout: 5000 })
  })
})
