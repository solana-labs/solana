import type { Config } from 'jest';

export default {
    displayName: 'ESLint',
    rootDir: '../../',
    runner: 'eslint',
    testMatch: ['<rootDir>src/**/*.ts', '<rootDir>internal/**'],
} as Config;
