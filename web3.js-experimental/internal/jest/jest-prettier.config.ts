import type { Config } from 'jest';

export default {
    displayName: 'Prettier',
    moduleFileExtensions: ['js', 'ts', 'json', 'md'],
    rootDir: '../../',
    runner: 'prettier',
    testMatch: ['<rootDir>README.md', '<rootDir>internal/**', '<rootDir>src/**', '<rootDir>*'],
} as Config;
