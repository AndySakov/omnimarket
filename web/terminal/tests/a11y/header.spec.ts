import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('global header has no serious or critical accessibility violations', async ({ page }) => {
  await page.goto('/')

  const results = await new AxeBuilder({ page }).analyze()
  const blockingViolations = results.violations.filter(
    (violation) => violation.impact === 'serious' || violation.impact === 'critical',
  )

  expect(blockingViolations).toEqual([])
})
