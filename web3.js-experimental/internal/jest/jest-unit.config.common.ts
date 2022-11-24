import type { Config } from 'jest';

export default {
    globals: {
        __DEV__: false,
    },
    rootDir: '../../',
    roots: ['<rootDir>src/'],
    setupFilesAfterEnv: ['<rootDir>internal/jest/setupFile.ts'],
    transform: {
        '^.+\\.(ts|js)$': [
            '@swc/jest',
            {
                jsc: {
                    target: 'es2020',
                },
            },
        ],
    },
} as Config;
