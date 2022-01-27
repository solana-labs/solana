module.exports = {
  roots: ["<rootDir>/src"],
  verbose: true,
  automock: false,
  resetMocks: false,
  setupFiles: [
    "./setupJest.js"
  ],
  testEnvironment: 'jest-environment-jsdom',
  testMatch: [
    "**/__tests__/**/*.+(ts|tsx|js)",
  ],
  transform: {
    "^.+\\.(t|j)sx?$": "ts-jest",
  },
  coveragePathIgnorePatterns: [
    "/node_modules/"
  ],
  moduleFileExtensions: ['ts', 'tsx', 'js', 'jsx', 'json', 'node'],
  moduleNameMapper: {
    "\\.(css|less)$": "identity-obj-proxy",
  }
};