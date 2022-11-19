// Clever obfuscation to prevent the build system from inlining the value of `NODE_ENV`
export const __DEV__ = /* @__PURE__ */ (() => process['en' + 'v'].NODE_ENV === 'development')();
