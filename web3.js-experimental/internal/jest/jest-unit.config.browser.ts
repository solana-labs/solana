import type { Config } from 'jest';
import commonConfig from './jest-unit.config.common';

export default {
    ...commonConfig,
    displayName: 'Unit Test (Browser)',
    globals: {
        ...commonConfig.globals,
        __BROWSER__: true,
        __NODEJS__: false,
        __REACTNATIVE__: false,
    },
    testEnvironment: 'jsdom',
    testEnvironmentOptions: {},
} as Config;
