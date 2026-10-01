import { expect, test } from '@playwright/test'

// #66: a guest is one click from a funded shadow balance, the session survives a reload, and
// signing out returns to the guest entry. Fixture source: MSW answers POST /v1/session.
test('a guest reaches a funded shadow balance in one click, keeps it across a reload, and signs out', async ({ page }) => {
  await page.goto('/')

  await page.getByRole('button', { name: 'Continue as guest' }).click()
  const account = page.getByRole('button', { name: /^Open account menu: guest, shadow balance \$3,496\.40$/ })
  await expect(account).toBeVisible()
  await expect(account).toContainText('$3,496.40 shadow')

  await page.reload()
  await expect(account).toBeVisible()
  await expect(page.getByRole('button', { name: 'Continue as guest' })).toHaveCount(0)

  await account.click()
  const panel = page.getByRole('dialog', { name: 'Account' })
  await expect(panel).toContainText('Shadow balance. Simulated funds for the demo')
  await expect(panel.getByRole('row', { name: /ETH 0\.9 \$2,250\.00/ })).toBeVisible()
  // The account topic's snapshot arrives over the fixture stream.
  await expect(panel.getByRole('status', { name: /^Shadow balances: Live\./ })).toBeVisible()

  await panel.getByRole('button', { name: 'Sign out' }).click()
  await expect(page.getByRole('button', { name: 'Continue as guest' })).toBeVisible()
  await page.reload()
  await expect(page.getByRole('button', { name: 'Continue as guest' })).toBeVisible()
})

test('the account menu works from the keyboard', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Continue as guest' }).focus()
  await page.keyboard.press('Enter')
  const account = page.getByRole('button', { name: /^Open account menu: guest/ })
  await expect(account).toBeFocused()

  await page.keyboard.press('Enter')
  const panel = page.getByRole('dialog', { name: 'Account' })
  await expect(panel).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(panel).toBeHidden()
  await expect(account).toBeFocused()

  await page.keyboard.press('Enter')
  await panel.getByRole('button', { name: 'Sign out' }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Continue as guest' })).toBeFocused()
})
