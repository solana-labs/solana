import React from 'react';
import ReactDOM from 'react-dom';
import './index.css';
import App from './App';
import * as serviceWorker from './serviceWorker';

import { waitReady, ed25519 } from './../../../';

waitReady().then(() => {
    const keyPair = ed25519.keypair.generate();
    const signature = ed25519.sign(keyPair.public, keyPair.secret, new TextEncoder().encode('message to encode'));
    console.log(signature);
});

ReactDOM.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
  document.getElementById('root')
);

// If you want your app to work offline and load faster, you can change
// unregister() to register() below. Note this comes with some pitfalls.
// Learn more about service workers: https://bit.ly/CRA-PWA
serviceWorker.unregister();
