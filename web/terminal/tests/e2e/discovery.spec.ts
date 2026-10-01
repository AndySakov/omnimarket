import { expect, test } from '@playwright/test'

test('keeps the terminal fixed while the token stream scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  // Fixture mode starts its mock worker before the app renders.
  await expect(page.locator('.discovery-table-scroll')).toBeVisible()

  const metrics = await page.evaluate(() => {
    const body = document.body
    const scrollRegion = document.querySelector<HTMLElement>('.discovery-table-scroll')

    return {
      documentFitsViewport: body.scrollHeight <= body.clientHeight + 1,
      tokenRegionScrolls: Boolean(scrollRegion && scrollRegion.scrollHeight > scrollRegion.clientHeight),
    }
  })

  expect(metrics.documentFitsViewport).toBe(true)
  expect(metrics.tokenRegionScrolls).toBe(true)
  await expect(page.locator('.token-avatar img')).toHaveCount(6)
  await expect(page.locator('.chain-rail__mark img')).toHaveCount(3)
})
