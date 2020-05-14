import * as wasm from './solana_sdk_wasm_bg.wasm';

const lTextDecoder = typeof TextDecoder === 'undefined' ? (0, module.require)('util').TextDecoder : TextDecoder;

let cachedTextDecoder = new lTextDecoder('utf-8', { ignoreBOM: true, fatal: true });

cachedTextDecoder.decode();

let cachegetUint8Memory0 = null;
function getUint8Memory0() {
    if (cachegetUint8Memory0 === null || cachegetUint8Memory0.buffer !== wasm.memory.buffer) {
        cachegetUint8Memory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachegetUint8Memory0;
}

function getStringFromWasm0(ptr, len) {
    return cachedTextDecoder.decode(getUint8Memory0().subarray(ptr, ptr + len));
}

const heap = new Array(32).fill(undefined);

heap.push(undefined, null, true, false);

let heap_next = heap.length;

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function getObject(idx) { return heap[idx]; }

function dropObject(idx) {
    if (idx < 36) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
    return instance.ptr;
}

let WASM_VECTOR_LEN = 0;

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1);
    getUint8Memory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

let cachegetInt32Memory0 = null;
function getInt32Memory0() {
    if (cachegetInt32Memory0 === null || cachegetInt32Memory0.buffer !== wasm.memory.buffer) {
        cachegetInt32Memory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachegetInt32Memory0;
}
/**
*/
export const StakeState = Object.freeze({ Uninitialized:0,Initialized:1,Delegated:2,RewardsPool:3, });
/**
*/
export class Authorized {

    static __wrap(ptr) {
        const obj = Object.create(Authorized.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_authorized_free(ptr);
    }
    /**
    * @returns {Pubkey}
    */
    get staker() {
        var ret = wasm.__wbg_get_authorized_staker(this.ptr);
        return Pubkey.__wrap(ret);
    }
    /**
    * @param {Pubkey} arg0
    */
    set staker(arg0) {
        _assertClass(arg0, Pubkey);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_authorized_staker(this.ptr, ptr0);
    }
    /**
    * @returns {Pubkey}
    */
    get withdrawer() {
        var ret = wasm.__wbg_get_authorized_withdrawer(this.ptr);
        return Pubkey.__wrap(ret);
    }
    /**
    * @param {Pubkey} arg0
    */
    set withdrawer(arg0) {
        _assertClass(arg0, Pubkey);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_authorized_withdrawer(this.ptr, ptr0);
    }
}
/**
*/
export class Delegation {

    static __wrap(ptr) {
        const obj = Object.create(Delegation.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_delegation_free(ptr);
    }
    /**
    * @returns {Pubkey}
    */
    get voterPubkey() {
        var ret = wasm.delegation_voter_pubkey(this.ptr);
        return Pubkey.__wrap(ret);
    }
    /**
    * @returns {number}
    */
    get stake() {
        var ret = wasm.delegation_stake(this.ptr);
        return ret;
    }
    /**
    * @returns {boolean}
    */
    isBootstrapStake() {
        var ret = wasm.delegation_isBootstrapStake(this.ptr);
        return ret !== 0;
    }
    /**
    * @returns {boolean}
    */
    isDeactivated() {
        var ret = wasm.delegation_isDeactivated(this.ptr);
        return ret !== 0;
    }
    /**
    * @returns {number}
    */
    get activationEpoch() {
        var ret = wasm.delegation_activation_epoch(this.ptr);
        return ret;
    }
    /**
    * @returns {number}
    */
    get deactivationEpoch() {
        var ret = wasm.delegation_deactivation_epoch(this.ptr);
        return ret;
    }
    /**
    * @returns {number}
    */
    get warmupCooldownRate() {
        var ret = wasm.delegation_warmup_cooldown_rate(this.ptr);
        return ret;
    }
}
/**
*/
export class Lockup {

    static __wrap(ptr) {
        const obj = Object.create(Lockup.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_lockup_free(ptr);
    }
    /**
    * custodian signature on a transaction exempts the operation from
    *  lockup constraints
    * @returns {Pubkey}
    */
    get custodian() {
        var ret = wasm.__wbg_get_lockup_custodian(this.ptr);
        return Pubkey.__wrap(ret);
    }
    /**
    * custodian signature on a transaction exempts the operation from
    *  lockup constraints
    * @param {Pubkey} arg0
    */
    set custodian(arg0) {
        _assertClass(arg0, Pubkey);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_lockup_custodian(this.ptr, ptr0);
    }
    /**
    * @returns {number}
    */
    get unixTimestamp() {
        var ret = wasm.lockup_unix_timestamp(this.ptr);
        return ret;
    }
    /**
    * @returns {number}
    */
    get epoch() {
        var ret = wasm.lockup_epoch(this.ptr);
        return ret;
    }
}
/**
*/
export class Meta {

    static __wrap(ptr) {
        const obj = Object.create(Meta.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_meta_free(ptr);
    }
    /**
    * @returns {Authorized}
    */
    get authorized() {
        var ret = wasm.__wbg_get_meta_authorized(this.ptr);
        return Authorized.__wrap(ret);
    }
    /**
    * @param {Authorized} arg0
    */
    set authorized(arg0) {
        _assertClass(arg0, Authorized);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_meta_authorized(this.ptr, ptr0);
    }
    /**
    * @returns {Lockup}
    */
    get lockup() {
        var ret = wasm.__wbg_get_meta_lockup(this.ptr);
        return Lockup.__wrap(ret);
    }
    /**
    * @param {Lockup} arg0
    */
    set lockup(arg0) {
        _assertClass(arg0, Lockup);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_meta_lockup(this.ptr, ptr0);
    }
    /**
    * @returns {number}
    */
    get rentExemptReserve() {
        var ret = wasm.meta_rent_exempt_reserve(this.ptr);
        return ret;
    }
}
/**
*/
export class Pubkey {

    static __wrap(ptr) {
        const obj = Object.create(Pubkey.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_pubkey_free(ptr);
    }
    /**
    * @returns {string}
    */
    toBase58() {
        try {
            wasm.pubkey_toBase58(8, this.ptr);
            var r0 = getInt32Memory0()[8 / 4 + 0];
            var r1 = getInt32Memory0()[8 / 4 + 1];
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_free(r0, r1);
        }
    }
}
/**
*/
export class Stake {

    static __wrap(ptr) {
        const obj = Object.create(Stake.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_stake_free(ptr);
    }
    /**
    * @returns {Delegation}
    */
    get delegation() {
        var ret = wasm.__wbg_get_stake_delegation(this.ptr);
        return Delegation.__wrap(ret);
    }
    /**
    * @param {Delegation} arg0
    */
    set delegation(arg0) {
        _assertClass(arg0, Delegation);
        var ptr0 = arg0.ptr;
        arg0.ptr = 0;
        wasm.__wbg_set_stake_delegation(this.ptr, ptr0);
    }
    /**
    * @returns {number}
    */
    get creditsObserved() {
        var ret = wasm.stake_credits_observed(this.ptr);
        return ret;
    }
}
/**
*/
export class StakeAccount {

    static __wrap(ptr) {
        const obj = Object.create(StakeAccount.prototype);
        obj.ptr = ptr;

        return obj;
    }

    free() {
        const ptr = this.ptr;
        this.ptr = 0;

        wasm.__wbg_stakeaccount_free(ptr);
    }
    /**
    * @returns {Meta | undefined}
    */
    get meta() {
        var ret = wasm.__wbg_get_stakeaccount_meta(this.ptr);
        return ret === 0 ? undefined : Meta.__wrap(ret);
    }
    /**
    * @param {Meta | undefined} arg0
    */
    set meta(arg0) {
        let ptr0 = 0;
        if (!isLikeNone(arg0)) {
            _assertClass(arg0, Meta);
            ptr0 = arg0.ptr;
            arg0.ptr = 0;
        }
        wasm.__wbg_set_stakeaccount_meta(this.ptr, ptr0);
    }
    /**
    * @returns {Stake | undefined}
    */
    get stake() {
        var ret = wasm.__wbg_get_stakeaccount_stake(this.ptr);
        return ret === 0 ? undefined : Stake.__wrap(ret);
    }
    /**
    * @param {Stake | undefined} arg0
    */
    set stake(arg0) {
        let ptr0 = 0;
        if (!isLikeNone(arg0)) {
            _assertClass(arg0, Stake);
            ptr0 = arg0.ptr;
            arg0.ptr = 0;
        }
        wasm.__wbg_set_stakeaccount_stake(this.ptr, ptr0);
    }
    /**
    * @returns {number}
    */
    get state() {
        var ret = wasm.__wbg_get_stakeaccount_state(this.ptr);
        return ret >>> 0;
    }
    /**
    * @param {number} arg0
    */
    set state(arg0) {
        wasm.__wbg_set_stakeaccount_state(this.ptr, arg0);
    }
    /**
    * @param {Uint8Array} data
    * @returns {StakeAccount}
    */
    static fromAccountData(data) {
        var ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        var ret = wasm.stakeaccount_fromAccountData(ptr0, len0);
        return StakeAccount.__wrap(ret);
    }
    /**
    * @returns {string}
    */
    displayState() {
        try {
            wasm.stakeaccount_displayState(8, this.ptr);
            var r0 = getInt32Memory0()[8 / 4 + 0];
            var r1 = getInt32Memory0()[8 / 4 + 1];
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_free(r0, r1);
        }
    }
}

export const __wbindgen_string_new = function(arg0, arg1) {
    var ret = getStringFromWasm0(arg0, arg1);
    return addHeapObject(ret);
};

export const __wbindgen_throw = function(arg0, arg1) {
    throw new Error(getStringFromWasm0(arg0, arg1));
};

export const __wbindgen_rethrow = function(arg0) {
    throw takeObject(arg0);
};

