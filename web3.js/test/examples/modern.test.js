import modern from '../../src/examples/modern';

const message =
  '\n' +
  '  \x1b[42m\x1b[30m                                         \n\x1b[0m' +
  '  \x1b[42m\x1b[30m  Thank you for using this boilerplate!  \n\x1b[0m' +
  '  \x1b[42m\x1b[30m                                         \n\x1b[0m' +
  '\n' +
  '  Getting started\n\n' +
  '  1. Clone the repo from github (https://github.com/eunikitin/modern-package-boilerplate.git)\n' +
  '  2. Inside the repo directory run npm install && rm -r .git && git init\n' +
  '  3. Update package.json with your information' +
  '\n';

test('Message on package usage', () => {
  expect(modern()).toBe(message);
});
