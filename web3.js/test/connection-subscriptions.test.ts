import chai from 'chai';
import {Client} from 'rpc-websockets';
import {stub, SinonStubbedInstance, SinonSpy, spy} from 'sinon';
import sinonChai from 'sinon-chai';

import {
  AccountChangeCallback,
  Commitment,
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
  const subscriptionMethodsConfig = {
    accountSubscribe: {
      getExpectedAlternateParams: () => [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {
          commitment: connection.commitment || 'finalized',
          encoding: 'base64',
        },
      ],
      getExpectedParams: () => [
        PublicKey.default.toBase58(),
        {
          commitment: connection.commitment || 'finalized',
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
          connection.commitment || 'finalized',
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
              space: 0,
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
      getExpectedAlternateParams: () => [
        {mentions: ['C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK']},
        {commitment: connection.commitment || 'finalized'},
      ],
      getExpectedParams: () => [
        {mentions: [PublicKey.default.toBase58()]},
        {commitment: connection.commitment || 'finalized'},
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
        return connection.onLogs(
          PublicKey.default,
          callback,
          connection.commitment || 'finalized',
        );
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
                'SBF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success',
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
      getExpectedAlternateParams: () => [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {commitment: connection.commitment || 'finalized', encoding: 'base64'},
      ],
      getExpectedParams: () => [
        PublicKey.default.toBase58(),
        {commitment: connection.commitment || 'finalized', encoding: 'base64'},
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
          connection.commitment || 'finalized',
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
                space: 0,
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
    rootSubscribe: {
      getExpectedAlternateParams: () => [],
      getExpectedParams: () => [],
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

    signatureSubscribe: {
      getExpectedAlternateParams: () => [
        'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK',
        {commitment: connection.commitment || 'finalized'},
      ],
      getExpectedParams: () => [
        PublicKey.default.toBase58(),
        {commitment: connection.commitment || 'finalized'},
      ],
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
          connection.commitment || 'finalized',
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
    slotSubscribe: {
      getExpectedAlternateParams: () => [],
      getExpectedParams: () => [],
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
      getExpectedAlternateParams: () => [],
      getExpectedParams: () => [],
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
  };
  beforeEach(() => {
    connection = new Connection(url);
    stubbedSocket = stubRpcWebSocket(connection);
  });
  afterEach(() => {
    stubbedSocket.close();
  });
  Object.entries(subscriptionMethodsConfig).forEach(
    ([
      subscriptionMethod,
      {
        getExpectedAlternateParams,
        getExpectedParams,
        publishNotificationForServerSubscriptionId,
        setupAlternateListener,
        setupListener,
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
              .withArgs(subscriptionMethod, getExpectedParams())
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
              getExpectedParams(),
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
                      getExpectedParams(),
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
              describe('then having the socket connection error', () => {
                beforeEach(() => {
                  stubbedSocket.emit(
                    'error',
                    new Error('A bad thing happened to the socket'),
                  );
                });
                describe('making another subscription while disconnected', () => {
                  beforeEach(() => {
                    stubbedSocket.call.resetHistory();
                    setupListener(spy());
                  });
                  it('does not issue an RPC call', () => {
                    expect(stubbedSocket.call).not.to.have.been.called;
                  });
                });
              });
              describe('then having the socket connection drop unexpectedly', () => {
                beforeEach(() => {
                  stubbedSocket.emit('close');
                });
                describe('making another subscription while disconnected', () => {
                  beforeEach(() => {
                    stubbedSocket.call.resetHistory();
                    setupListener(spy());
                  });
                  it('does not issue an RPC call', () => {
                    expect(stubbedSocket.call).not.to.have.been.called;
                  });
                });
                describe('upon the socket connection reopening', () => {
                  let fatalPriorUnubscribe: () => void;
                  beforeEach(() => {
                    fatalPriorUnubscribe = fatalUnsubscribe;
                    stubbedSocket.call.resetHistory();
                    stubbedSocket.emit('open');
                  });
                  it('does not result in a new unsubscription request being made to the RPC', () => {
                    expect(stubbedSocket.call).not.to.have.been.called;
                  });
                  describe('then upon the prior unsubscribe fataling (eg. because its timeout triggers)', () => {
                    beforeEach(async () => {
                      stubbedSocket.call.resetHistory();
                      await fatalPriorUnubscribe();
                    });
                    it('does not result in a new unsubscription request being made to the RPC', () => {
                      expect(stubbedSocket.call).not.to.have.been.called;
                    });
                  });
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
                    .withArgs(subscriptionMethod, getExpectedAlternateParams())
                    .resolves(secondServerSubscriptionId);
                  alternateListenerCallback = spy();
                  setupAlternateListener(alternateListenerCallback);
                });
                it('results in a second subscription request being made to the RPC', () => {
                  expect(stubbedSocket.call).to.have.been.calledWithExactly(
                    subscriptionMethod,
                    getExpectedAlternateParams(),
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
                getExpectedParams(),
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
              let fatalPriorSubscription: () => void;
              beforeEach(() => {
                fatalPriorSubscription = fatalSubscription;
                stubbedSocket.call.resetHistory();
                stubbedSocket.emit('open');
              });
              it('results in a new subscription request being made to the RPC', () => {
                expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
                  subscriptionMethod,
                  getExpectedParams(),
                );
              });
              describe('then upon the prior subscription fataling (eg. because its timeout triggers)', () => {
                beforeEach(async () => {
                  stubbedSocket.call.resetHistory();
                  await fatalPriorSubscription();
                });
                it('does not result in a new subscription request being made to the RPC', () => {
                  expect(stubbedSocket.call).not.to.have.been.called;
                });
                describe('once the new subscription has been acknowledged by the server', () => {
                  beforeEach(async () => {
                    stubbedSocket.call.resetHistory();
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
                });
              });
            });
          });
        });
      });
    },
  );
  /**
   * Special case.
   * After a signature is processed, RPCs automatically dispose of the
   * subscription on the server side. This test asserts that RPC
   * unsubscribe request are only made before such a subscription has
   * received a notification which it knows to be final and indicative
   * that the RPC has auto-disposed the subscription.
   *
   * NOTE: There is a proposal to eliminate this special case, here:
   * https://github.com/solana-labs/solana/issues/18892
   */
  describe('auto-disposing subscriptions', () => {
    let clientSubscriptionId: number;
    const serverSubscriptionId = 0;
    const testSignature = 'C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK';
    const getExpectedParams = () => [
      testSignature,
      {commitment: connection.commitment || 'finalized'},
    ];
    // This type of notification *is* indicative of auto-disposal.
    const FINAL_NOTIFICATION_RESULT = {
      context: {slot: 11},
      value: {err: null},
    };
    // This type of notification is *not* indicative of auto-disposal.
    const NON_FINAL_NOTIFICATION_RESULT = {
      context: {slot: 11},
      value: 'receivedSignature',
    };
    beforeEach(() => {
      stubbedSocket.call
        .withArgs('signatureSubscribe', getExpectedParams())
        .resolves(serverSubscriptionId);
      clientSubscriptionId = connection.onSignature(testSignature, spy());
    });
    describe('before an auto-disposing subscription has published any notification', () => {
      describe('then unsubscribing the listener', () => {
        beforeEach(async () => {
          stubbedSocket.call.resetHistory();
          await connection.removeSignatureListener(clientSubscriptionId);
        });
        it('results in an unsubscribe request being made to the RPC', () => {
          expect(stubbedSocket.call).to.have.been.calledWith(
            'signatureUnsubscribe',
            [serverSubscriptionId],
          );
        });
      });
    });
    describe('after an auto-disposing subscription has published a non-final notification', () => {
      beforeEach(() => {
        stubbedSocket.emit('signatureNotification', {
          subscription: serverSubscriptionId,
          result: NON_FINAL_NOTIFICATION_RESULT,
        });
      });
      it('should not result in an unsubscribe request being made to the RPC', () => {
        expect(stubbedSocket.call).not.to.have.been.calledWith(
          'signatureUnsubscribe',
          [serverSubscriptionId],
        );
      });
      describe('then unsubscribing the listener', () => {
        beforeEach(async () => {
          stubbedSocket.call.resetHistory();
          await connection.removeSignatureListener(clientSubscriptionId);
        });
        it('results in an unsubscribe request being made to the RPC', () => {
          expect(stubbedSocket.call).to.have.been.calledWith(
            'signatureUnsubscribe',
            [serverSubscriptionId],
          );
        });
      });
    });
    describe('after an auto-disposing subscription has published its final notification', () => {
      beforeEach(() => {
        stubbedSocket.emit('signatureNotification', {
          subscription: serverSubscriptionId,
          result: FINAL_NOTIFICATION_RESULT,
        });
      });
      it('should not result in an unsubscribe request being made to the RPC', () => {
        expect(stubbedSocket.call).not.to.have.been.calledWith(
          'signatureUnsubscribe',
          [serverSubscriptionId],
        );
      });
      describe('then unsubscribing the listener', () => {
        beforeEach(async () => {
          stubbedSocket.call.resetHistory();
          await connection.removeSignatureListener(clientSubscriptionId);
        });
        it('should not result in an unsubscribe request being made to the RPC', () => {
          expect(stubbedSocket.call).not.to.have.been.called;
        });
      });
    });
  });
  [
    undefined, // Let `Connection` use the default commitment
    'processed' as Commitment, // Override `Connection's` commitment
  ].forEach((maybeOverrideCommitment: Commitment | undefined) => {
    describe(`given a Connection with ${
      maybeOverrideCommitment
        ? `its commitment overriden to \`${maybeOverrideCommitment}\``
        : 'an unspecified commitment override'
    }`, () => {
      Object.entries(subscriptionMethodsConfig).forEach(
        ([
          subscriptionMethod,
          {
            getExpectedParams,
            setupListenerWithDefaultableParamsSetToTheirDefaults,
            setupListenerWithDefaultsOmitted,
          },
        ]) => {
          beforeEach(() => {
            connection = new Connection(url, maybeOverrideCommitment);
            stubbedSocket = stubRpcWebSocket(connection);
          });
          afterEach(() => {
            stubbedSocket.close();
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
                  getExpectedParams(),
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
  });
  describe('during state machine updates', () => {
    beforeEach(() => {
      stubbedSocket.connect.callsFake(() => {});
      stubbedSocket.close.callsFake(() => {});
    });
    afterEach(() => {
      stubbedSocket.emit('close');
    });
    /**
     * This is a regression test for the case described here:
     * https://github.com/solana-labs/solana/pull/24473#discussion_r858437090
     *
     * Essentially, you want to make sure that the state processor, as it recurses
     * always processes the latest version of every subscription. Depending on how
     * you craft the loop inside the processor, you can end up in this situation.
     *
     * 1A (pending subscription with zero callbacks; gets deleted then recurses)
     *  L 2B (pending subscription; transitions to subscribing and makes network call)
     * 2A (old version of subscription 2; transitions again and makes 2nd network call)
     *
     * The fact that subscription 2 made two network calls is the bug there.
     * What you want is this:
     *
     * 1A (pending subscription with zero callbacks; gets deleted then recurses)
     *  L 2B (pending subscription; transitions to subscribing and makes network call)
     * 2A (now in the subscribing state; skipped by the processor)
     *
     * Below is a test that tries to replicate this exact scenario.
     */
    it('the processor always operates over the most up-to-date state of a given subscription', () => {
      // Add two subscriptions.
      const clientSubscriptionIdA = connection.onAccountChange(
        new PublicKey('C2jDL4pcwpE2pP5EryTGn842JJUJTcurPGZUquQjySxK'),
        () => {},
      );
      connection.onAccountChange(
        new PublicKey('27Y78XJXG9A13pnPajrB1VYU6EF8uNSoojPZBmhKsi8C'),
        () => {},
      );
      // Then remove the first one before the connection opens.
      connection.removeAccountChangeListener(clientSubscriptionIdA);
      // Then open the connection.
      stubbedSocket.emit('open');
      // Despite recursion inside the state machine, ensure that the second
      // subscription only makes *one* connection attempt.
      expect(stubbedSocket.call).to.have.been.calledOnceWithExactly(
        'accountSubscribe',
        [
          '27Y78XJXG9A13pnPajrB1VYU6EF8uNSoojPZBmhKsi8C',
          {commitment: 'finalized', encoding: 'base64'},
        ],
      );
    });
  });
});
