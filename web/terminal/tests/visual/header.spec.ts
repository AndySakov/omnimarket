import { expect, test } from '@playwright/test'

test.describe('global header visual baselines', () => {
  test('desktop shell', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 })
    await page.goto('/?visual=1')
    // The fixture stream connects before the baseline is taken, so it never catches "Connecting".
    await expect(page.getByRole('status', { name: /^Market data: Fixtures/ })).toBeVisible()

    // The engine status moves every fixture block; its values are masked so the baseline holds still.
    await expect(page.getByRole('button', { name: /^Engine: Fixtures\./ })).toBeVisible()
    await expect(page.locator('.global-header')).toHaveScreenshot('header-desktop.png', {
      mask: [page.locator('.engine-status__item strong')],
    })
  })

  test('mobile navigation menu', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/?visual=1')
    await page.getByRole('button', { name: 'Open navigation menu' }).click()

    await expect(page.locator('.global-header')).toHaveScreenshot('header-mobile-menu.png')
  })
})
