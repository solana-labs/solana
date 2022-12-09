import rpc from '../rpc';

describe('rpc', () => {
    if (__BROWSER__) {
        it('logs stuff in the browser', () => {
            const spy = jest.spyOn(console, 'log');
            rpc();
            expect(spy).toHaveBeenCalledWith(`This is the browser fork of \`doThing()\``);
        });
    }
    if (__NODEJS__) {
        it('logs stuff in node', () => {
            const spy = jest.spyOn(console, 'log');
            rpc();
            expect(spy).toHaveBeenCalledWith(`This is the node fork of \`doThing()\``);
        });
    }
    it.todo('does something useful');
});
