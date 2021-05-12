module.exports = {
  env: {
    browser: true,
    es6: true,
    node: true,
    mocha: true,
  },
  extends: [
    'eslint:recommended',
    'plugin:import/errors',
    'plugin:import/warnings',
    'plugin:import/typescript',
  ],
  parser: '@typescript-eslint/parser',
  parserOptions: {
    sourceType: 'module',
    ecmaVersion: 8,
  },
  plugins: ['@typescript-eslint'],
  rules: {
    '@typescript-eslint/no-unused-vars': ['error'],
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
    'no-trailing-spaces': ['error'],
    'no-unused-vars': 'off',
    quotes: [
      'error',
      'single',
      {avoidEscape: true, allowTemplateLiterals: true},
    ],
    'require-await': ['error'],
    semi: ['error', 'always'],
  },
};
