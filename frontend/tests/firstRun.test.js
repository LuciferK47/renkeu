import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte'

vi.mock('../src/lib/api/client.js', () => ({
  api: {
    getConfig: vi.fn(),
    saveConfig: vi.fn(),
    setApiKey: vi.fn(),
    getSetup: vi.fn(),
    finishSetup: vi.fn(),
    prepareTex: vi.fn(),
    getLocalModels: vi.fn()
  },
  APIError: class extends Error {}
}))

import { api } from '../src/lib/api/client.js'
import WelcomeDialog from '../src/components/WelcomeDialog.svelte'
import LocalModelPicker from '../src/components/LocalModelPicker.svelte'

const server = (over = {}) => ({ ai_enabled: false, ai_local: false, model: 'llama-3.3-70b-versatile', key_source: 'none', env_overridden: [], ...over })
const ollama = { running: true, models: [{ name: 'qwen2.5:7b', size_bytes: 4.7e9 }, { name: 'llama3.2:3b', size_bytes: 2e9 }] }

beforeEach(() => {
  vi.clearAllMocks()
  api.getConfig.mockResolvedValue(server())
  api.prepareTex.mockResolvedValue({ ok: true, message: null })
  api.finishSetup.mockResolvedValue({ done: true })
  api.getLocalModels.mockResolvedValue(ollama)
})

describe('LocalModelPicker', () => {
  it('lists the installed models and sets the chosen one as the assistant model', async () => {
    api.saveConfig.mockResolvedValue(server({ ai_enabled: true, ai_local: true, model: 'llama3.2:3b' }))
    const onchoose = vi.fn()
    render(LocalModelPicker, { props: { server: server(), onchoose } })
    const select = await screen.findByLabelText('Local model')
    expect([...select.options].map((o) => o.value)).toEqual(['qwen2.5:7b', 'llama3.2:3b'])

    await fireEvent.change(select, { target: { value: 'llama3.2:3b' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Use this model' }))
    await waitFor(() => expect(api.saveConfig).toHaveBeenCalledWith({ model: 'llama3.2:3b' }))
    expect(onchoose).toHaveBeenCalled()
  })

  it('says how to start Ollama when it is not running, and can check again', async () => {
    api.getLocalModels.mockResolvedValue({ running: false, models: [] })
    render(LocalModelPicker, { props: { server: server() } })
    expect(await screen.findByText(/Ollama is not running/)).toBeTruthy()

    api.getLocalModels.mockResolvedValue(ollama)
    await fireEvent.click(screen.getByRole('button', { name: 'Check again' }))
    expect(await screen.findByLabelText('Local model')).toBeTruthy()
  })

  it('says to pull a model when Ollama has none', async () => {
    api.getLocalModels.mockResolvedValue({ running: true, models: [] })
    render(LocalModelPicker, { props: { server: server() } })
    expect(await screen.findByText(/ollama pull/)).toBeTruthy()
  })

  it('shows which model is in use, and does not offer to pick it again', async () => {
    render(LocalModelPicker, { props: { server: server({ ai_enabled: true, ai_local: true, model: 'qwen2.5:7b' }) } })
    expect((await screen.findByRole('status')).textContent).toContain('qwen2.5:7b')
    expect(screen.getByRole('button', { name: 'Use this model' }).disabled).toBe(true)
  })
})

describe('WelcomeDialog', () => {
  it('prepares TeX on its own and reports when it is ready', async () => {
    render(WelcomeDialog)
    expect(await screen.findByText(/Ready\. Compiling works/)).toBeTruthy()
    expect(api.prepareTex).toHaveBeenCalledTimes(1)
  })

  it('shows why the TeX download failed and retries on request', async () => {
    api.prepareTex.mockResolvedValueOnce({ ok: false, message: 'No connection.' })
    render(WelcomeDialog)
    expect(await screen.findByText('No connection.')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(await screen.findByText(/Ready\. Compiling works/)).toBeTruthy()
    expect(api.prepareTex).toHaveBeenCalledTimes(2)
  })

  it('works with no assistant, and remembers that it was finished', async () => {
    const onclose = vi.fn()
    render(WelcomeDialog, { props: { onclose } })
    await fireEvent.click(screen.getByRole('button', { name: 'Start writing' }))
    await waitFor(() => expect(onclose).toHaveBeenCalled())
    expect(api.finishSetup).toHaveBeenCalledTimes(1)
    expect(api.saveConfig).not.toHaveBeenCalled()
    expect(api.setApiKey).not.toHaveBeenCalled()
  })

  it('still closes when the server cannot save the marker', async () => {
    api.finishSetup.mockRejectedValue(new Error('read-only disk'))
    const onclose = vi.fn()
    render(WelcomeDialog, { props: { onclose } })
    await fireEvent.click(screen.getByRole('button', { name: 'Start writing' }))
    await waitFor(() => expect(onclose).toHaveBeenCalled())
  })

  it('offers the local model list when that choice is picked', async () => {
    render(WelcomeDialog)
    await fireEvent.click(screen.getByLabelText(/A model on this computer/))
    expect(await screen.findByLabelText('Local model')).toBeTruthy()
  })

  it('stores a typed key through the write-only endpoint and clears the box', async () => {
    api.setApiKey.mockResolvedValue(server({ ai_enabled: true, key_source: 'file' }))
    render(WelcomeDialog)
    await fireEvent.click(screen.getByLabelText(/My own key/))
    const box = await screen.findByLabelText('Groq API key')
    await fireEvent.input(box, { target: { value: ' gsk_x ' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save key' }))
    await waitFor(() => expect(api.setApiKey).toHaveBeenCalledWith('gsk_x'))
    await waitFor(() => expect(screen.getByLabelText('Groq API key').value).toBe(''))
  })

  it('starts on the choice that is already in effect', async () => {
    api.getConfig.mockResolvedValue(server({ ai_enabled: true, ai_local: true, model: 'qwen2.5:7b' }))
    render(WelcomeDialog)
    await waitFor(() => expect(screen.getByLabelText(/A model on this computer/).checked).toBe(true))
  })
})
