// Correctness rules only. Formatting is not enforced: the codebase has never
// been machine-formatted and a style rule would flag most of it for nothing.
import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';

export default [
  { ignores: ['dist/', 'node_modules/'] },
  js.configs.recommended,
  ...svelte.configs['flat/recommended'],
  {
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.node }
    },
    rules: {
      eqeqeq: ['error', 'always'],
      // State here is replaced, never mutated in place (new Set(old).add(x)),
      // which is what makes it reactive; the rule's advice does not apply.
      'svelte/prefer-svelte-reactivity': 'off',
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'no-unused-vars': ['warn', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }]
    }
  },
  {
    // Runes modules use `$state` and friends, which are compiler globals.
    files: ['**/*.svelte.js', '**/*.svelte'],
    languageOptions: {
      globals: { $state: 'readonly', $derived: 'readonly', $effect: 'readonly', $props: 'readonly', $bindable: 'readonly', $inspect: 'readonly', $host: 'readonly' }
    }
  },
  {
    files: ['tests/**'],
    languageOptions: { globals: { ...globals.vitest, vi: 'readonly', describe: 'readonly', it: 'readonly', expect: 'readonly', beforeEach: 'readonly', afterEach: 'readonly' } }
  }
];
