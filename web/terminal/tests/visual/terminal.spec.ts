import { expect, test, type Page } from '@playwright/test'

async function waitForStableSurface(page: Page) {
  await page.evaluate(async () => {
    await document.fonts.ready
    await Promise.all([...document.images].map((image) => image.complete ? Promise.resolve() : new Promise((resolve) => {
      image.addEventListener('load', resolve, { once: true })
      image.addEventListener('error', resolve, { once: true })
    })))
  })
}

test.describe('terminal typography reference baselines', () => {
  test('discover wide reference composition', async ({ page }) => {
    await page.setViewportSize({ width: 1918, height: 744 })
    await page.goto('/')
    await page.locator('.discovery-table-shell').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('discover-wide-1918x744.png')
  })

  test('discover standard desktop composition', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 })
    await page.goto('/')
    await page.locator('.discovery-table-shell').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('discover-desktop-1440x900.png')
  })

  test('discover compact desktop composition', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 })
    await page.goto('/')
    await page.locator('.discovery-table-shell').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('discover-compact-1280x720.png')
  })

  test('discover mobile composition', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await page.locator('.discovery-table-shell').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('discover-mobile-390x844.png')
  })

  test('token workspace desktop composition', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 })
    await page.goto('/')
    await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()
    await page.locator('.token-page').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('token-workspace-desktop-1440x900.png')
  })

  test('token workspace mobile composition', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()
    await page.locator('.token-page').waitFor({ state: 'visible' })
    await waitForStableSurface(page)

    await expect(page).toHaveScreenshot('token-workspace-mobile-390x844.png')
  })
})
