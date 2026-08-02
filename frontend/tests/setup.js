// Vitest setup file
import '@testing-library/jest-dom'

// jsdom implements no matchMedia, and ThemeToggle asks for the OS colour
// preference on mount. Without this, rendering anything containing it throws an
// unhandled error -- tests still pass, which is worse: real failures would hide
// among the noise.
if (!window.matchMedia) {
  window.matchMedia = (query) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })
}

// Monaco and the layout code both observe element size; jsdom has neither API.
if (!window.ResizeObserver) {
  window.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
}
