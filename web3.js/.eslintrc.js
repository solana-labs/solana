module.exports = {
  env: {
    browser: true,
    es6: true,
    node: true,
  },
  extends: [
    'eslint:recommended',
    'plugin:import/errors',
    'plugin:import/warnings',
    'plugin:flowtype/recommended',
  ],
  parser: 'babel-eslint',
  parserOptions: {
    sourceType: 'module',
    ecmaVersion: 8,
  },
  plugins: ['flowtype'],
  rules: {
    'no-trailing-spaces': ['error'],
    'import/first': ['error'],
    'import/no-commonjs': ['error'],
    'import/order': [
      'error',
      {
        groups: [
          ['internal', 'external', 'builtin'],
          ['index', 'sibling', 'parent'],
        ],
        'newlines-between': 'always',
      },
    ],
    indent: [
      'error',
      2,
      {
        MemberExpression: 1,
        SwitchCase: 1,
      },
    ],
    'linebreak-style': ['error', 'unix'],
    'no-console': [0],
    quotes: [
      'error',
      'single',
      {avoidEscape: true, allowTemplateLiterals: true},
    ],
    'require-await': ['error'],
    semi: ['error', 'always'],
    'flowtype/generic-spacing': [0],
  },

  // Used to lint the TypeScript type declaration file
  overrides: [
    {
      files: ['*.d.ts'],
      parser: '@typescript-eslint/parser',
      plugins: ['@typescript-eslint'],
      extends: [
        'eslint:recommended',
        'plugin:@typescript-eslint/eslint-recommended',
        'plugin:@typescript-eslint/recommended',
      ],
    },
  ],
};
