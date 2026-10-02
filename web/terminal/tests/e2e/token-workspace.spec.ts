import { expect, test } from '@playwright/test'

test('opens the token workspace and prepares a mocked trade review', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')

  await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()
  await expect(page.locator('.token-page')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NovaSet' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Trades' })).toBeVisible()
  await expect(page.getByText('Fixture stream connected', { exact: true })).toBeVisible()
  await expect(page.getByText('0.00036 ETH', { exact: true })).toBeVisible()
  await expect(page.getByText('$14.8M', { exact: true }).first()).toBeVisible()
  await expect(page.getByRole('link', { name: 'Aerodrome 0.30% 0xpool…9a10' })).toBeVisible()
  await expect(page.getByRole('link', { name: '0x9c21…a810' })).toHaveAttribute('href', /basescan\.org\/tx/)
  await expect(page.getByRole('region', { name: 'Safety evidence' })).toBeVisible()
  await expect(page.getByText('Not checked yet', { exact: true }).first()).toBeVisible()

  await page.getByRole('tab', { name: 'Holders' }).click()
  await expect(page.getByRole('heading', { name: 'Holder distribution' })).toBeVisible()

  await page.getByLabel('Amount', { exact: true }).fill('0.25')
  await page.getByRole('button', { name: 'Review trade' }).click()
  await expect(page.getByText('Review ready')).toBeVisible()

  const metrics = await page.evaluate(() => ({
    tokenPageOwnsScroll: Boolean(document.querySelector<HTMLElement>('.token-page')?.scrollHeight),
    tradePanelVisible: Boolean(document.querySelector('.trade-panel')),
  }))
  expect(metrics.tokenPageOwnsScroll).toBe(true)
  expect(metrics.tradePanelVisible).toBe(true)
})

test('resolves a token workspace from the public address route', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/base/token/0x7b3f8a42')

  await expect(page).toHaveURL(/\/base\/token\/0x7b3f8a42$/)
  await expect(page.getByRole('main', { name: 'NovaSet token workspace' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NovaSet' })).toBeVisible()
})

test('stacks the token workspace on mobile without losing the trade panel', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()

  await expect(page.locator('.trade-panel')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Review trade' })).toBeVisible()
  const metrics = await page.evaluate(() => ({
    documentFitsViewport: document.body.scrollWidth <= document.body.clientWidth + 1,
    tokenPageOwnsScroll: Boolean(document.querySelector<HTMLElement>('.token-page')?.scrollHeight),
  }))
  expect(metrics.documentFitsViewport).toBe(true)
  expect(metrics.tokenPageOwnsScroll).toBe(true)
})

test('loads a distinct fixture snapshot when the chart interval changes', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()

  const hourlyInterval = page.getByRole('tab', { name: '1h', exact: true })
  await hourlyInterval.click()
  await expect(hourlyInterval).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByText('1h candles', { exact: true })).toBeVisible()
  await expect(page.getByRole('img', { name: /NovaSet candlestick price chart \(1h\)/ })).toBeVisible()
})
