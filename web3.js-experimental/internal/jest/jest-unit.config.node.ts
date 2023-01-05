import { Config } from 'jest';
import commonConfig from './jest-unit.config.common';

export default {
    ...commonConfig,
    displayName: 'Unit Test (Node)',
    globals: {
        ...commonConfig.globals,
        __BROWSER__: false,
        __NODEJS__: true,
        __REACTNATIVE__: false,
    },
} as Config;
