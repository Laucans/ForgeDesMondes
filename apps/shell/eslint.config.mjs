// The lint of the shell's TypeScript: the recommended rules, type-aware —
// a floating promise or an unsafe `any` is an error, not a style choice.
import js from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist/', 'node_modules/', 'build.mjs', 'eslint.config.mjs'] },
  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  {
    // node:test's `describe` and `test` return promises the runner collects itself, and a fake
    // `fetch` is async without awaiting.
    files: ['**/*.test.ts'],
    rules: { '@typescript-eslint/no-floating-promises': 'off', '@typescript-eslint/require-await': 'off' },
  },
  { languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } } },
);
