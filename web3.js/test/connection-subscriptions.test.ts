import chai from 'chai';
import {Client} from 'rpc-websockets';
import {stub, SinonStubbedInstance, SinonSpy, spy} from 'sinon';
import sinonChai from 'sinon-chai';

import {
  AccountChangeCallback,
  Connection,
  LogsCallback,
  ProgramAccountChangeCallback,
  PublicKey,
  RootChangeCallback,
  SignatureResultCallback,
  SlotChangeCallback,
  SlotUpdateCallback,
} from '../src';
import {url} from './url';

chai.use(sinonChai);
const expect = chai.expect;

function stubRpcWebSocket(
  connection: Connection,
): SinonStubbedInstance<Client> {
  const socket = connection._rpcWebSocket;
  let mockOpen = false;
  stub(socket, 'connect').callsFake(() => {
    if (!mockOpen) {
      mockOpen = true;
      connection._rpcWebSocket.emit('open');
    }
  });
  stub(socket, 'close').callsFake(() => {
    if (mockOpen) {
      mockOpen = false;
      connection._rpcWebSocket.emit('close');
    }
  });
  stub(socket, 'call');
  return socket as SinonStubbedInstance<Client>;
}

describe('Subscriptions', () => {
  let connection: Connection;
  let stubbedSocket: SinonStubbedInstance<Client>;
  beforeEach(() => {
    connection = new Connection(url);
    stubbedSocket = stubRpcWebSocket(connection);
  });
  afterEach(() => {
    stubbedSocket.close();
  });
  Object.entries({
    accountSubscribe: {
      expectedAlternateParams: [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {
          commitment: 'finalized',
          encoding: 'base64',
        },
      ],
      expectedParams: [
        PublicKey.default.toBase58(),
        {
          commitment: 'finalized',
          encoding: 'base64',
        },
      ],
      setupAlternateListener(callback: AccountChangeCallback): number {
        return connection.onAccountChange(
          new PublicKey('C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK'),
          callback,
        );
      },
      setupListener(callback: AccountChangeCallback): number {
        return connection.onAccountChange(PublicKey.default, callback);
      },
      setupListenerWithDefaultsOmitted(
        callback: AccountChangeCallback,
      ): number {
        return connection.onAccountChange(PublicKey.default, callback);
      },
      setupListenerWithDefaultableParamsSetToTheirDefaults(
        callback: AccountChangeCallback,
      ): number {
        return connection.onAccountChange(
          PublicKey.default,
          callback,
          'finalized',
        );
      },
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('accountNotification', {
          subscription: serverSubscriptionId,
          result: {
            context: {slot: 11},
            value: {
              data: Buffer.from(''),
              executable: false,
              lamports: 0,
              owner: PublicKey.default.toBase58(),
              rentEpoch: 0,
            },
          },
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeAccountChangeListener>
      ) {
        return connection.removeAccountChangeListener(...args);
      },
    },
    logsSubscribe: {
      expectedAlternateParams: [
        {mentions: ['C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK']},
        {commitment: 'finalized'},
      ],
      expectedParams: [
        {mentions: [PublicKey.default.toBase58()]},
        {commitment: 'finalized'},
      ],
      setupAlternateListener(callback: LogsCallback): number {
        return connection.onLogs(
          new PublicKey('C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK'),
          callback,
        );
      },
      setupListener(callback: LogsCallback): number {
        return connection.onLogs(PublicKey.default, callback);
      },
      setupListenerWithDefaultsOmitted(callback: LogsCallback): number {
        return connection.onLogs(PublicKey.default, callback);
      },
      setupListenerWithDefaultableParamsSetToTheirDefaults(
        callback: LogsCallback,
      ): number {
        return connection.onLogs(PublicKey.default, callback, 'finalized');
      },
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('logsNotification', {
          subscription: serverSubscriptionId,
          result: {
            context: {slot: 11},
            value: {
              err: null,
              logs: [
                'BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success',
              ],
              signature:
                '5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv',
            },
          },
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeOnLogsListener>
      ) {
        return connection.removeOnLogsListener(...args);
      },
    },
    programSubscribe: {
      expectedAlternateParams: [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {commitment: 'finalized', encoding: 'base64'},
      ],
      expectedParams: [
        PublicKey.default.toBase58(),
        {commitment: 'finalized', encoding: 'base64'},
      ],
      setupAlternateListener(callback: ProgramAccountChangeCallback): number {
        return connection.onProgramAccountChange(
          new PublicKey('C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK'),
          callback,
        );
      },
      setupListener(callback: ProgramAccountChangeCallback): number {
        return connection.onProgramAccountChange(PublicKey.default, callback);
      },
      setupListenerWithDefaultsOmitted(
        callback: ProgramAccountChangeCallback,
      ): number {
        return connection.onProgramAccountChange(PublicKey.default, callback);
      },
      setupListenerWithDefaultableParamsSetToTheirDefaults(
        callback: ProgramAccountChangeCallback,
      ): number {
        return connection.onProgramAccountChange(
          PublicKey.default,
          callback,
          'finalized',
        );
      },
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('programNotification', {
          subscription: serverSubscriptionId,
          result: {
            context: {slot: 11},
            value: {
              pubkey: PublicKey.default.toBase58(),
              account: {
                data: Buffer.from(''),
                executable: false,
                lamports: 0,
                owner: PublicKey.default.toBase58(),
                rentEpoch: 0,
              },
            },
          },
        });
      },
      teardownListener(
        ...args: Parameters<
          typeof connection.removeProgramAccountChangeListener
        >
      ) {
        return connection.removeProgramAccountChangeListener(...args);
      },
    },
    signatureSubscribe: {
      expectedAlternateParams: [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {commitment: 'finalized'},
      ],
      expectedParams: [PublicKey.default.toBase58(), {commitment: 'finalized'}],
      setupAlternateListener(callback: SignatureResultCallback): number {
        return connection.onSignature(
          new PublicKey(
            'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
          ).toBase58(),
          callback,
        );
      },
      setupListener(callback: SignatureResultCallback): number {
        return connection.onSignature(PublicKey.default.toBase58(), callback);
      },
      setupListenerWithDefaultsOmitted(
        callback: SignatureResultCallback,
      ): number {
        return connection.onSignature(PublicKey.default.toBase58(), callback);
      },
      setupListenerWithDefaultableParamsSetToTheirDefaults(
        callback: SignatureResultCallback,
      ): number {
        return connection.onSignature(
          PublicKey.default.toBase58(),
          callback,
          'finalized',
        );
      },
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('signatureNotification', {
          subscription: serverSubscriptionId,
          result: {
            context: {slot: 11},
            value: {err: null},
          },
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeSignatureListener>
      ) {
        return connection.removeSignatureListener(...args);
      },
    },
    rootSubscribe: {
      expectedAlternateParams: [],
      expectedParams: [],
      setupAlternateListener: undefined,
      setupListener(callback: RootChangeCallback): number {
        return connection.onRootChange(callback);
      },
      setupListenerWithDefaultsOmitted: undefined,
      setupListenerWithDefaultableParamsSetToTheirDefaults: undefined,
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('rootNotification', {
          subscription: serverSubscriptionId,
          result: 101,
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeRootChangeListener>
      ) {
        return connection.removeRootChangeListener(...args);
      },
    },
    slotSubscribe: {
      expectedAlternateParams: [],
      expectedParams: [],
      setupAlternateListener: undefined,
      setupListener(callback: SlotChangeCallback): number {
        return connection.onSlotChange(callback);
      },
      setupListenerWithDefaultsOmitted: undefined,
      setupListenerWithDefaultableParamsSetToTheirDefaults: undefined,
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('slotNotification', {
          subscription: serverSubscriptionId,
          result: {parent: 1, slot: 2, root: 0},
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeSlotChangeListener>
      ) {
        return connection.removeSlotChangeListener(...args);
      },
    },
    slotsUpdatesSubscribe: {
      expectedAlternateParams: [],
      expectedParams: [],
      setupAlternateListener: undefined,
      setupListener(callback: SlotUpdateCallback): number {
        return connection.onSlotUpdate(callback);
      },
      setupListenerWithDefaultsOmitted: undefined,
      setupListenerWithDefaultableParamsSetToTheirDefaults: undefined,
      publishNotificationForServerSubscriptionId(
        socket: Client,
        serverSubscriptionId: number,
      ) {
        socket.emit('slotsUpdatesNotification', {
          subscription: serverSubscriptionId,
          result: {
            type: 'root',
            slot: 0,
            timestamp: 322992000000,
          },
        });
      },
      teardownListener(
        ...args: Parameters<typeof connection.removeSlotUpdateListener>
      ) {
        return connection.removeSlotUpdateListener(...args);
      },
    },
  }).forEach(
    ([
      subscriptionMethod,
      {
        expectedAlternateParams,
        expectedParams,
        publishNotificationForServerSubscriptionId,
        setupAlternateListener,
        setupListener,
        setupListenerWithDefaultableParamsSetToTheirDefaults,
        setupListenerWithDefaultsOmitted,
        teardownListener,
      },
    ]) => {
      describe(`The \`${subscriptionMethod}\` RPC method`, () => {
        describe('attaching the first notification listener', () => {
          let clientSubscriptionId: number;
          let listenerCallback: SinonSpy;
          let acknowledgeSubscription = (
            // eslint-disable-next-line @typescript-eslint/no-unused-vars
            _serverSubscriptionId: number,
          ) => {
            expect.fail(
              'Expected a function to have been assigned to `acknowledgeSubscription` in the test.',
            );
          };
          let fatalSubscription = () => {
            expect.fail(
              'Expected a function to have been assigned to `fatalSubscription` in the test.',
            );
          };
          const serverSubscriptionId = 0;
          beforeEach(() => {
            stubbedSocket.call
              .withArgs(subscriptionMethod, expectedParams)
              .callsFake(
                () =>
                  // Defer the acknowledgement.
                  new Promise<number>((resolve, reject) => {
                    acknowledgeSubscription = resolve;
                    fatalSubscription = reject;
                  }),
              );
            listenerCallback = spy();
            clientSubscriptionId = setupListener(listenerCallback);
          });
          it('results in a subscription request being made to the RPC', () => {
            expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
              subscriptionMethod,
              expectedParams,
            );
          });
          describe('then unsubscribing that listener before the subscription has been acknowledged by the server', () => {
            beforeEach(async () => {
              stubbedSocket.call.resetHistory();
              await teardownListener(clientSubscriptionId);
            });
            describe('once the subscription has been acknowledged by the server', () => {
              beforeEach(async () => {
                await acknowledgeSubscription(serverSubscriptionId);
              });
              it('results in the subscription being torn down immediately', () => {
                expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                  subscriptionMethod.replace('Subscribe', 'Unsubscribe'),
                  [serverSubscriptionId],
                );
              });
            });
          });
          describe('once the subscription has been acknowledged by the server', () => {
            beforeEach(async () => {
              await acknowledgeSubscription(serverSubscriptionId);
            });
            describe('when a notification is published', () => {
              beforeEach(() => {
                publishNotificationForServerSubscriptionId(
                  stubbedSocket,
                  serverSubscriptionId,
                );
              });
              it('fires the listener callback', () => {
                expect(listenerCallback).to.have.been.calledOnce;
              });
            });
            describe('then unsubscribing that listener', () => {
              let acknowledgeUnsubscribe = () => {
                expect.fail(
                  'Expected a function to have been assigned to `acknowledgeUnsubscribe` in the test',
                );
              };
              let fatalUnsubscribe = () => {
                expect.fail(
                  'Expected a function to have been assigned to `fatalUnsubscribe` in the test',
                );
              };
              beforeEach(() => {
                stubbedSocket.call.resetHistory();
                stubbedSocket.call
                  .withArgs(
                    subscriptionMethod.replace('Subscribe', 'Unsubscribe'),
                    [serverSubscriptionId],
                  )
                  .callsFake(
                    () =>
                      // Defer the acknowledgement.
                      new Promise<void>((resolve, reject) => {
                        acknowledgeUnsubscribe = resolve;
                        fatalUnsubscribe = reject;
                      }),
                  );
                teardownListener(clientSubscriptionId);
              });
              it('results in an unsubscribe request being made to the RPC', () => {
                expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                  subscriptionMethod.replace('Subscribe', 'Unsubscribe'),
                  [serverSubscriptionId],
                );
              });
              describe('if a new listener is added before the unsubscribe is acknowledged by the server', () => {
                beforeEach(() => {
                  stubbedSocket.call.resetHistory();
                  setupListener(spy());
                });
                describe('once that unsubscribe is acknowledged by the server', () => {
                  beforeEach(async () => {
                    await acknowledgeUnsubscribe();
                  });
                  it('results in a new subscription request being made to the RPC', () => {
                    expect(
                      stubbedSocket.call,
                    ).to.have.been.calledOnceWithExactly(
                      subscriptionMethod,
                      expectedParams,
                    );
                  });
                });
              });
              describe('when a notification is published before the unsubscribe is acknowledged by the server', () => {
                beforeEach(() => {
                  publishNotificationForServerSubscriptionId(
                    stubbedSocket,
                    serverSubscriptionId,
                  );
                });
                it('does not fire the listener callback', () => {
                  expect(listenerCallback).not.to.have.been.called;
                });
              });
              describe('if that unsubscribe throws an exception', () => {
                beforeEach(async () => {
                  stubbedSocket.call.resetHistory();
                  await fatalUnsubscribe();
                });
                it('results in a retry unsubscribe request being made to the RPC', () => {
                  expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                    subscriptionMethod.replace('Subscribe', 'Unsubscribe'),
                    [serverSubscriptionId],
                  );
                });
              });
            });
            describe('attaching a second notification listener with the same params', () => {
              let secondListenerCallback: SinonSpy;
              beforeEach(() => {
                stubbedSocket.call.resetHistory();
                secondListenerCallback = spy();
                setupListener(secondListenerCallback);
              });
              it('does not result in a second subscription request to the RPC', () => {
                expect(stubbedSocket.call).not.to.have.been.called;
              });
              describe('when a notification is published', () => {
                beforeEach(() => {
                  publishNotificationForServerSubscriptionId(
                    stubbedSocket,
                    serverSubscriptionId,
                  );
                });
                it("fires the first listener's callback", () => {
                  expect(listenerCallback).to.have.been.calledOnce;
                });
                it("fires the second listener's callback", () => {
                  expect(secondListenerCallback).to.have.been.calledOnce;
                });
              });
              describe('then unsubscribing the first listener', () => {
                beforeEach(async () => {
                  stubbedSocket.call.resetHistory();
                  await teardownListener(clientSubscriptionId);
                });
                it('does not result in an unsubscribe request being made to the RPC', () => {
                  expect(stubbedSocket.call).not.to.have.been.called;
                });
                describe('when a notification is published', () => {
                  beforeEach(() => {
                    publishNotificationForServerSubscriptionId(
                      stubbedSocket,
                      serverSubscriptionId,
                    );
                  });
                  it("does not fire the first listener's callback", () => {
                    expect(listenerCallback).not.to.have.been.called;
                  });
                  it("fires the second listener's callback", () => {
                    expect(secondListenerCallback).to.have.been.calledOnce;
                  });
                });
              });
            });
            if (setupAlternateListener) {
              describe('attaching a second notification listener with different params', () => {
                let alternateListenerCallback: SinonSpy;
                const secondServerSubscriptionId = 1;
                beforeEach(() => {
                  stubbedSocket.call
                    .withArgs(subscriptionMethod, expectedAlternateParams)
                    .resolves(secondServerSubscriptionId);
                  alternateListenerCallback = spy();
                  setupAlternateListener(alternateListenerCallback);
                });
                it('results in a second subscription request being made to the RPC', () => {
                  expect(stubbedSocket.call).to.have.been.calledWithExactly(
                    subscriptionMethod,
                    expectedAlternateParams,
                  );
                });
                describe('when a notification for the first subscription is published', () => {
                  beforeEach(() => {
                    publishNotificationForServerSubscriptionId(
                      stubbedSocket,
                      serverSubscriptionId,
                    );
                  });
                  it("fires the first listener's callback", () => {
                    expect(listenerCallback).to.have.been.called;
                  });
                  it("does not fire the second listener's callback", () => {
                    expect(alternateListenerCallback).not.to.have.been.called;
                  });
                });
                describe('when a notification for the second subscription is published', () => {
                  beforeEach(() => {
                    publishNotificationForServerSubscriptionId(
                      stubbedSocket,
                      secondServerSubscriptionId,
                    );
                  });
                  it("does not fire the first listener's callback", () => {
                    expect(listenerCallback).not.to.have.been.called;
                  });
                  it("fires the second listener's callback", () => {
                    expect(alternateListenerCallback).to.have.been.called;
                  });
                });
              });
            }
          });
          describe('if that subscription throws an exception', () => {
            beforeEach(async () => {
              stubbedSocket.call.resetHistory();
              await fatalSubscription();
            });
            it('results in a retry subscription request being made to the RPC', () => {
              expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                subscriptionMethod,
                expectedParams,
              );
            });
          });
          describe('then having the socket connection drop unexpectedly', () => {
            beforeEach(() => {
              stubbedSocket.emit('close');
            });
            describe('then unsubscribing that listener', () => {
              beforeEach(async () => {
                await teardownListener(clientSubscriptionId);
              });
              describe('upon the socket connection reopening', () => {
                beforeEach(() => {
                  stubbedSocket.call.resetHistory();
                  stubbedSocket.emit('open');
                });
                it('does not result in a new subscription request being made to the RPC', () => {
                  expect(stubbedSocket.call).not.to.have.been.called;
                });
              });
            });
            describe('upon the socket connection reopening', () => {
              beforeEach(() => {
                stubbedSocket.call.resetHistory();
                stubbedSocket.emit('open');
              });
              it('results in a new subscription request being made to the RPC', () => {
                expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                  subscriptionMethod,
                  expectedParams,
                );
              });
            });
          });
        });
      });
      if (
        setupListenerWithDefaultsOmitted &&
        setupListenerWithDefaultableParamsSetToTheirDefaults
      ) {
        describe('making a subscription with defaulted params omitted', () => {
          beforeEach(() => {
            setupListenerWithDefaultsOmitted(spy());
          });
          it('results in a subscription request being made to the RPC', () => {
            expect(stubbedSocket.call).to.have.been.calledWithExactly(
              subscriptionMethod,
              expectedParams,
            );
          });
          describe('then making the same subscription with the defaultable params set to their defaults', () => {
            beforeEach(() => {
              stubbedSocket.call.resetHistory();
              setupListenerWithDefaultableParamsSetToTheirDefaults(spy());
            });
            it('does not result in a subscription request being made to the RPC', () => {
              expect(stubbedSocket.call).not.to.have.been.called;
            });
          });
        });
      }
    },
  );
});
