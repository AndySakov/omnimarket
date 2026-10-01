// Live Discover (#65) in the browser, on the fixture source: its feed adds a new pool every few
// seconds and moves prices every second.
import { expect, test, type Page } from '@playwright/test'

const NEW_POOL = 'Brine (fixture)'

async function names(page: Page): Promise<string[]> {
  return page.locator('tbody .token-name').allTextContents()
}

test('a pool created during the session appears without a reload', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  await expect(page.locator('tbody .token-name')).toHaveCount(6)
  await expect(page.getByText(NEW_POOL, { exact: true })).toBeHidden()

  await expect(page.locator('tbody .token-name').first()).toHaveText(NEW_POOL, { timeout: 15_000 })
  await expect(page.locator('tbody .token-name')).toHaveCount(7)
})

test('the row under the pointer does not jump: new rows wait until the pointer leaves', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  await expect(page.locator('tbody .token-name')).toHaveCount(6)

  await page.locator('tbody tr').nth(2).hover()
  const held = await names(page)
  await expect(page.getByText(/Order held while you're in the table · 1 new waiting/)).toBeVisible({ timeout: 15_000 })
  expect(await names(page)).toEqual(held)

  await page.mouse.move(5, 5)
  await expect(page.locator('tbody .token-name').first()).toHaveText(NEW_POOL)
})
