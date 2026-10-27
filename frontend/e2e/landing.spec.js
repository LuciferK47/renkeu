import { test, expect } from '@playwright/test'

// The landing page is the first thing a new visitor sees. These tests use the
// real server, so "engine online" is real; the offline case blocks the request.

test.describe('landing page', () => {
  test('a first visit shows the page, with the engine online and one way into the editor', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1 })).toContainText('Write LaTeX.')
    await expect(page.getByRole('status').filter({ hasText: 'Engine online' })).toBeVisible()
    const open = page.getByRole('button', { name: 'Open editor' }).first()
    await expect(open).toBeVisible()
    await open.click()
    await expect(page.getByRole('tree')).toBeVisible()
  })

  test('without an engine the main action is to get the app, and the chip says so', async ({ page }) => {
    await page.route('**/api/config', (route) => route.abort())
    await page.goto('/#/landing')
    await expect(page.getByRole('status').filter({ hasText: 'Engine offline' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Open editor' })).toHaveCount(0)
    await expect(page.getByRole('link', { name: 'Get Renkeu' }).first()).toBeVisible()
  })

  test('someone who has opened the editor before goes straight back to it', async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem('latex-ai:opened-editor', 'true'))
    await page.goto('/')
    await expect(page.getByRole('tree')).toBeVisible()
  })

  test('the review gate in the repair loop opens only when Apply is pressed', async ({ page }) => {
    await page.goto('/?poster#/landing')
    await page.getByRole('button', { name: /Review/ }).click()
    await expect(page.getByRole('button', { name: 'Apply 1 change' })).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('The gate stays shut until you press Apply.')).toBeVisible()
    await page.getByRole('button', { name: 'Apply 1 change' }).click()
    await expect(page.getByText(/This is a demo: nothing was written/)).toBeVisible()
  })

  test('with ?poster the scene is a still image, not a canvas', async ({ page }) => {
    await page.goto('/?poster#/landing')
    await expect(page.locator('.scene img').first()).toBeAttached()
    await expect(page.locator('.scene canvas')).toHaveCount(0)
  })

  test('nothing the page says is hidden behind the animation: pausing it keeps every control', async ({ page }) => {
    await page.goto('/#/landing')
    const pause = page.getByRole('button', { name: 'Pause the animation' })
    await pause.click()
    await expect(page.getByRole('button', { name: 'Resume the animation' })).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByRole('link', { name: 'View source on GitHub' }).first()).toBeVisible()
  })

  test('the copy button reports what happened', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']).catch(() => {})
    await page.goto('/#/landing')
    await page.locator('#install').scrollIntoViewIfNeeded()
    await page.getByRole('button', { name: 'Copy the Docker command' }).click()
    await expect(page.getByText(/Copied\.|Could not copy/)).toBeVisible()
  })
})
