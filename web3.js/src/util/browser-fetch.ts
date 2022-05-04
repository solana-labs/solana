/**
 * This is the module that gets included in browser environments.
 * We simply use the existing implementation of `fetch`.
 */
export default /*#__PURE__*/ (function () {
  return window.fetch;
})();
