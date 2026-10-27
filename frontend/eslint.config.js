import js from '@eslint/js'
import { plugin as shadcn } from '@shadcn/lint'
import jsxA11y from 'eslint-plugin-jsx-a11y'
import reactHooks from 'eslint-plugin-react-hooks'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  { ignores: ['dist', 'src/api/schema.d.ts', 'playwright-report', 'test-results'] },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [js.configs.recommended, ...tseslint.configs.recommended, jsxA11y.flatConfigs.recommended],
    languageOptions: { ecmaVersion: 2023, globals: globals.browser },
    plugins: { 'react-hooks': reactHooks, shadcn },
    rules: {
      // shadcn design-system rules (discovery via components.json + src/index.css tokens).
      'shadcn/no-raw-colors': 'error',
      'shadcn/no-unknown-classes': 'error',
      'shadcn/require-static-classes': 'error',
      'shadcn/no-arbitrary-values': 'error',
      'shadcn/no-inline-styles': 'error',
      // no-restyle is off: it forbids even layout classes (w-44, ml-auto) on primitives; see docs/frontend.md.
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      // Labels are associated via htmlFor on shadcn <Label>, which the rule cannot see.
      'jsx-a11y/label-has-associated-control': 'off',
      // Design-system rules: colours come from semantic tokens, never raw palette classes or hex values.
      'no-restricted-syntax': [
        'error',
        {
          selector: 'Literal[value=/\\b(bg|text|border|ring|outline|fill|stroke)-(red|green|blue|yellow|amber|orange|emerald|sky|indigo|violet|purple|pink|rose|slate|gray|zinc|neutral|stone)-\\d{2,3}\\b/]',
          message: 'Use semantic colour tokens (bg-primary, text-destructive, bg-success-soft, …) instead of raw palette colours.',
        },
        {
          selector: 'Literal[value=/\\b(bg|text|border)-\\[#[0-9a-fA-F]{3,8}\\]/]',
          message: 'Hard-coded hex colours are not allowed; add a token in src/index.css instead.',
        },
      ],
    },
  },
  {
    // Generated shadcn primitives are kept as upstream ships them.
    files: ['src/components/ui/**'],
    rules: {
      'jsx-a11y/heading-has-content': 'off',
      'jsx-a11y/anchor-has-content': 'off',
      'no-restricted-syntax': 'off',
      'shadcn/no-arbitrary-values': 'off',
      'shadcn/no-restyle': 'off',
      'shadcn/no-unknown-classes': 'off',
    },
  },
  {
    files: ['e2e/**', '*.config.ts'],
    languageOptions: { globals: globals.node },
  },
)
