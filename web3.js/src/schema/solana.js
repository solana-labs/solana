const resultFactory = (name, resultSchema, definitions = {}) => ({
    "$id": name,
    "anyOf": [
        { "$ref": "#/definitions/success" },
        { "$ref": "#/definitions/error" }
    ],
    "definitions": {
        ...definitions,
        "common": {
            required: [ "id", "jsonrpc" ],
            properties: {
                "id": { type: [ "string"  ] },
                "jsonrpc": { "enum": [ "2.0" ] }
            }
        },
        [name]: resultSchema,
        "success": {
            "allOf" : [
            { "$ref": "#/definitions/common" },
            {
                required: [ "result" ], properties: { "result": { "$ref": `#/definitions/${name}` }, }
            }
        ]},
        "error": {
            "allOf" : [
                { "$ref": "#/definitions/common" },
                {
                    required: [ "error" ],
                    properties: {
                        "error": {
                            properties: {
                                "code": {type: "integer" },
                                "message": { type: "string" }
                            }
                        }
                    }
                }
            ]
        }
    }
});

const contextFactory = (name, valueSchema) => {
    return resultFactory(name, {
        required: [ "context", "value" ],
        properties: {
            context: { required: [ "slot" ], properties: { "slot": { type: "integer" } } },
            value: valueSchema,
        }
    })
};

const notificationFactory = (name, valueSchema) => {
    return  {
        "$id": name,
        "description": `A JSON RPC 2.0 response for ${name}`,
        "oneOf": [
            { "$ref": "#/definitions/notification" }
        ],
        "definitions": {
            [name]: valueSchema,
            "notification": {
                required: ["subscription", "result"],
                properties: {
                    "subscription": { type: "number"},
                    "result": {
                        required: ["context", "value"],
                        properties: {
                            "context": { 
                                required: ["slot"],
                                properties: { "slot": { type: "number" } } 
                            },
                            "value": { "$ref": `#/definitions/${name}` }
                        }
                    }
                }
            }
        }
    };
};

const accountInfo = {
    required: [ "executable", "lamports", "owner", "data" ],
    properties: {
        "data": { type: "object" },
        "executable": { type: "boolean" },
        "lamports": { type: "integer" },
        "owner": { type: "string" },
        "rentEpoch": { type: "integer" },
    }
};

const programAccountInfo= {
    required: ['pubkey', 'account'],
    properties: {
        pubkey: { type: 'number' },
        account: accountInfo,
    }
}

const parsedAccountInfo = {
    required: [ "executable", "lamports", "owner", "data" ],
    properties: {
        "data": {
            required: [ "space", "program", "parsed" ],
            properties: {
                "parsed": { type: "object" },
                "program": { type: "string" },
                "space": { type: "integer" },
            }
        },
        "executable": { type: "boolean" },
        "lamports": { type: "integer" },
        "owner": { type: "string" },
        "rentEpoch": { type: "integer" },
    }
};

const parsedProgramAccountInfo = {
    required: ['pubkey', 'account'],
    properties: {
        pubkey: { type: 'number' },
        account: parsedAccountInfo,
    }
}

const confirmedTransaction = {
    required: ['signatures', 'message'], 
    properties: {
        signatures: {
            type: 'array',
            items: { type: 'string' }
        },
        message: {
            required: ['recentBlockhash', 'header', 'instructions'],
            properties: {
                recentBlockhash: { type: 'string' },
                header: {
                    properties: {
                        numRequiredSignatures: { type: 'number' },
                        numReadonlySignedAccounts: { type: 'number' },
                        numReadonlyUnsignedAccounts: { type: 'number' },
                    }
                },
                instructions: {
                    type: 'array', 
                    items: {
                        properties: {
                            accounts: { type: 'array', items: { type: 'number' } },
                            data: { type: 'string' },
                            programIdIndex: { type: 'number' },
                            programId: { type: 'string' },
                            parsed: { type: 'object' },
                            program: { type: 'string' }
                        }
                    }
                },
                accountKeys: { type: 'array', items: { type: 'string' } }
            }
        }
    }
};

const confirmedTransactionMeta = {
    properties: {
        err: { type: 'object' },
        fee: { type: 'number' },
        preBalances: { type: 'array', items: { type: 'number'} },
        postBalances: { type: 'array', items: { type: 'number'} },
    }
}

module.exports = [
    {
        ['$id']: 'info',
        required: ['name'],
        properties: {
            name: { type: 'string' },
            website: { type: 'string' },
            details: { type: 'string' },
            keybaseUsername: { type: 'string' },
        }
    },
    resultFactory('booleanResult', {
        type: "boolean"
    }),
    resultFactory('numberResult', {
        type: "number"
    }),
    resultFactory('stringResult', {
        type: "string"
    }),
    contextFactory('numberContextResult', {
        type: ["number", "null"]
    }),
    resultFactory('stringArray', {
        type: "array",
        items: {
            type: "string"
        }
    }),
    resultFactory('version', {
        required: [ "solana-core" ],
        properties: {
            "solana-core": { type: "string" },
            "feature-set": { type: "number" },
        }
    }),
    contextFactory('largestAccounts', {
        required: [ "lamports", "address" ],
        properties: {
            "lamports": { "type" : "number" },
            "address": { "type" : "string" },
        }
    }),
    resultFactory('stakeActivation', { 
        required: [ "state", "active", "inactive" ],
        properties: {
            "state": { "enum": [ 'active','inactive','activating','deactivating', ] },
            "active": { type: "integer" },
            "inactive": { type: "integer" },
        }
    }),
    contextFactory('accountInfo', accountInfo),
    contextFactory('parsedAccountInfo', parsedAccountInfo),
    contextFactory('parsedAccounts', {
        type: "array",
        items: {
            required: ["account", "pubkey"],
            properties: {
                "pubkey": { type: "string" },
                "account": { ...parsedAccountInfo }
            }
        }
    }),
    resultFactory('inflationGovernor', {
        required: ["foundation","foundationTerm","initial","taper","terminal"],
        properties: {
            "foundation": { type: "number" },
            "foundationTerm": { type: "number" },
            "initial": { type: "number" },
            "taper": { type: "number" },
            "terminal": { type: "number" },
        }
    }),
    resultFactory('epochInfo', {
        required: ["epoch", "slotIndex", "slotsInEpoch", "absoluteSlot"],
        properties: {
            "epoch": { type: "number" },
            "slotIndex": { type: "number" },
            "slotsInEpoch": { type: "number" },
            "absoluteSlot": { type: "number" },
            "blockHeight": { type: "number" },
        }
    }),
    resultFactory('epochSchedule', {
        required: ["slotsPerEpoch", "leaderScheduleSlotOffset", "warmup", "firstNormalEpoch", "firstNormalSlot"],
        properties: {
            "slotsPerEpoch": { type: "number" },
            "leaderScheduleSlotOffset": { type: "number" },
            "warmup": { type: "boolean" },
            "firstNormalEpoch": { type: "number" },
            "firstNormalSlot": { type: "number" },
        }
    }),
    contextFactory('tokenAccountBalance', {
        required: [ "amount", "uiAmount", "decimals" ],
        properties: {
            "amount": { type: "string" },
            "uiAmount": { type: "integer" },
            "decimals": { type: "integer" },
        }
    }),
    resultFactory('leaderSchedule', {
        "additionalProperties": {
            type: "array",
            items: {
                type: "number"
            }
        }
    }),
    resultFactory('confirmedSignaturesForAddress2', {
        type: "array",
        items: {
            required: ["signature", "slot"],
            properties: {
                "signature": { type: "string" },
                "slot": { type: "number" },
                "err": { type: "object" },
                "memo": { type: "string" }
            }
        }
    }),
    resultFactory('clusterNodes', {
        type: "array",
        items: {
            required: ["pubkey"],
            properties: {
                "pubkey": { type: "string" },
                "gossip": { type: "string" },
                "tpu": { type: "string" },
                "rpc": { type: "string" },
                "version": { type: "string" },
            }
        }
    }),
    contextFactory('recentBlockhash', {
        required: [ "blockhash", "feeCalculator" ],
        properties: {
            "blockhash": { type: "string" },
            "feeCalculator": { 
                required: [ "lamportsPerSignature" ],
                properties: {
                    "lamportsPerSignature": { type: "number"}
                }
             },
        }
    }),
    contextFactory('feeCalculator', {
        properties: {
            "feeCalculator": { 
                required: [ "lamportsPerSignature" ],
                properties: {
                    "lamportsPerSignature": { type: "number"}
                }
             },
        }
    }),
    resultFactory('confirmedBlock', {
        required: ['blockhash', 'previousBlockhash', 'parentSlot', 'transactions'], 
        properties: {
            blockhash: { type: 'string' },
            previousBlockhash: { type: 'string' },
            parentSlot: { type: 'number' },
            transactions: {
                type: 'array', 
                items: {
                    required: ['transaction', 'meta'], 
                    properties: {
                        transaction: confirmedTransaction,
                        meta: confirmedTransactionMeta,
                    }
                }
            }, 
            rewards: {
                type: 'array',
                items: {
                    requiried: ['pubkey', 'lamports'],
                    properties: {
                        pubkey: { type: 'string' },
                        lamports: { type: 'number' },
                    }
                }
            }
        }
    }),
    contextFactory('supply', {
        required: [],
        properties: {
            total: { type: 'number' },
            circulating: { type: 'number' },
            nonCirculating: { type: 'number' },
            nonCirculatingAccounts: {
                type: 'array', 
                items: { type: 'string' }
            },
        }
    }),
    contextFactory('tokenLargestAccounts', {
        type: 'array',
        items: {
            required: ['address', 'amount', 'uiAmount', 'decimals',],
            properties: {
                address: { type: 'string'} ,
                amount: { type: 'string'} ,
                uiAmount: { type: 'number'} ,
                decimals: { type: 'number'} ,
            }
        }
    }),
    contextFactory('tokenAccountByOwner', {
        type: 'array',
        items: {
            required: ['pubkey', 'account',],
            properties: {
                pubkey: { type: 'string'} ,
                account: { 
                    required: [ "executable", "lamports", "owner", "data" ],
                    properties: {
                        executable: { type: 'boolean' },
                        owner: { type: 'string' },
                        lamports: { type: 'number' },
                        data: { type: 'string' },
                        rentEpoch: { type: 'number' },
                    }
                },
            }
        }
    }),
    contextFactory('signatureStatuses', {
        type: 'array',
        items: {
            required: ['slot'],
            properties: {
                slot: { type: 'number' },
                confirmations: { type: 'number' },
                err: { type: 'object' }
            }
        }
    }),
    resultFactory('parsedConfirmedTransaction', {
        required: ['slot', 'transaction', 'meta'],
        properties: {
            slot: { type: 'number'},
            transaction: confirmedTransaction,
            meta: confirmedTransactionMeta,
        }
    }),
    resultFactory('voteAccounts', {
        properties: {
            current: {
                type: 'array',
                items: {
                    "$ref": `#/definitions/voteAccount`
                }
            },
            delinquent: {
                type: 'array',
                items: {
                    "$ref": `#/definitions/voteAccount`
                }
            }
        }
    }, {
        voteAccount: {
            required: ['votePubkey','nodePubkey','activatedStake', 'epochCredits', 'epochVoteAccount','commission','lastVote'],
            properties: {
                votePubkey: { type: 'string' },
                nodePubkey: { type: 'string' },
                activatedStake: { type: 'number' },
                epochVoteAccount: { type: 'boolean' },
                epochCredits: {
                    type: 'array',
                    items: [
                        { "type" : "number"}, 
                        { "type" : "number"}, 
                        { "type" : "number"}, 
                    ],
                    additionalItems: false,
                    minItems: 3,
                },
                commission: { type: 'number' },
                lastVote: { type: 'number' },
                rootSlot: { type: 'number' },
            }
        }
    }),
    contextFactory('simulateTransaction', {
        properties: {
            err: {
                oneOf: [
                    { type: 'string' },
                    { type: 'object' }
                ]
            },
            logs: {
                type: 'array',
                items: { type: 'string' },
            }
        }
    }),
    contextFactory('parsedTokenAccountsByOwner', {
        type: 'array',
        items: {
            required: ['pubkey', 'account',],
            properties: {
                pubkey: { type: 'string'} ,
                account: { 
                    required: [ "executable", "lamports", "owner", "data" ],
                    properties: {
                        executable: { type: 'boolean' },
                        owner: { type: 'string' },
                        lamports: { type: 'number' },
                        data: { 
                            required: ['program', 'parsed', 'space'],
                            properties: {
                                program: { type: 'string' },
                                parsed: { type: 'object' },
                                space: { type: 'number' },
                            }
                         },
                        rentEpoch: { type: 'number' },
                    }
                },
            }
        }
    }),
    resultFactory('confirmedTransaction', {
        required: ['transaction', 'meta', 'slot'], 
        properties: {
            slot: { type: 'number' },
            transaction: confirmedTransaction,
            meta: confirmedTransactionMeta,
        }
    }),
    resultFactory('programAccounts', {
        type: 'array',
        items: programAccountInfo,
    }),
    resultFactory('parsedProgramAccounts', {
        type: 'array',
        items: parsedProgramAccountInfo,
    }),
    notificationFactory('accountNotification', {
        ...accountInfo
    }),
    notificationFactory('programAccountNotification', {
        ...programAccountInfo
    }),
    notificationFactory('slotNotification', {
        required: ['parent', 'slot', 'root'],
        properties: {
            parent: { type: 'number' },
            slot: { type: 'number' },
            root: { type: 'number' },
        }
    }),
    notificationFactory('signatureNotification', {
        properties: {
            parent: { type: 'number' },
            slot: { type: 'number' },
            root: { type: 'number' },
        }
    }),
    notificationFactory('signatureStatusNotification', {
        properties: {
            err: { type: 'object' }
        }
    }),
    notificationFactory('rootNotification', {
        type: 'number'
    }),
];
