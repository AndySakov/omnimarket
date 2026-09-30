import { expect, test } from '@playwright/test'

test.describe('global header visual baselines', () => {
  test('desktop shell', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 })
    await page.goto('/')

    await expect(page.locator('.global-header')).toHaveScreenshot('header-desktop.png')
  })

  test('mobile navigation menu', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await page.getByRole('button', { name: 'Open navigation menu' }).click()

    await expect(page.locator('.global-header')).toHaveScreenshot('header-mobile-menu.png')
  })
})
