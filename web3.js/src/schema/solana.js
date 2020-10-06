const resultFactory = (name, resultSchema) => ({
    "$id": name,
    "description": `A JSON RPC 2.0 response for ${name}`,
    "oneOf": [
        { "$ref": "#/definitions/success" },
        { "$ref": "#/definitions/error" }
    ],
    "definitions": {
        "common": {
            "required": [ "id", "jsonrpc" ],
            "not": { "required": [ "result", "error" ] },
            "properties": {
                "id": { "type": [ "string"  ] },
                "jsonrpc": { "enum": [ "2.0" ] }
            }
        },
        [name]: resultSchema,
        "success": {
            "allOf" : [
            { "$ref": "#/definitions/common" },
            {
                "required": [ "result" ], "properties": { "result": { "$ref": `#/definitions/${name}` }, }
            }
        ]},
        "error": {
            "allOf" : [
                { "$ref": "#/definitions/common" },
                {
                    "required": [ "error" ],
                    "properties": {
                        "error": {
                            "properties": {
                                "code": {"type": "integer" },
                                "message": { "type": "string" }
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
        "required": [ "context", "value" ],
        "properties": {
            "context": { "required": [ "slot" ], "properties": { "slot": { "type": "integer" } } },
            "value": valueSchema,
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
                "required": ["subscription", "result"],
                "properties": {
                    "subscription": { "type": "number"},
                    "result": {
                        "required": ["context", "value"],
                        "properties": {
                            "context": { 
                                "required": ["slot"],
                                "properties": { "slot": { "type": "number" } } 
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
    "required": [ "executable", "lamports", "owner", "data" ],
    "properties": {
        "data": { "type": "object" },
        "executable": { "type": "boolean" },
        "lamports": { "type": "integer" },
        "owner": { "type": "string" },
        "rentEpoch": { "type": "integer" },
    }
};

const parsedAccountInfo = {
    "required": [ "executable", "lamports", "owner", "data" ],
    "properties": {
        "data": {
            "required": [ "space", "program", "parsed" ],
            "properties": {
                "parsed": { "type": "object" },
                "program": { "type": "string" },
                "space": { "type": "integer" },
            }
        },
        "executable": { "type": "boolean" },
        "lamports": { "type": "integer" },
        "owner": { "type": "string" },
        "rentEpoch": { "type": "integer" },
    }
};

module.exports = [
    resultFactory('booleanResult', {
        "type": "boolean"
    }),
    resultFactory('numberResult', {
        "type": "number"
    }),
    resultFactory('stringResult', {
        "type": "string"
    }),
    resultFactory('stringArray', {
        "type": "array",
        "items": {
            "type": "string"
        }
    }),
    resultFactory('version', {
        "required": [ "solana-core" ],
        "properties": {
            "solana-core": { "type": "string" },
            "feature-set": { "type": "number" },
        }
    }),
    contextFactory('largestAccounts', {
        "required": [ "lamports", "address" ],
        "properties": {
            "lamports": { "type" : "number" },
            "address": { "type" : "string" },
        }
    }),
    resultFactory('stakeActivation', { 
        "required": [ "state", "active", "inactive" ],
        "properties": {
            "state": { "enum": [ 'active','inactive','activating','deactivating', ] },
            "active": { "type": "integer" },
            "inactive": { "type": "integer" },
        }
    }),
    contextFactory('accountInfo', accountInfo),
    contextFactory('parsedAccountInfo', parsedAccountInfo),
    contextFactory('parsedAccounts', {
        "type": "array",
        "items": {
            "required": ["account", "pubkey"],
            "properties": {
                "pubkey": { "type": "string" },
                "account": { ...parsedAccountInfo }
            }
        }
    }),
    resultFactory('inflationGovernor', {
        "required": ["foundation","foundationTerm","initial","taper","terminal"],
        "properties": {
            "foundation": { "type": "number" },
            "foundationTerm": { "type": "number" },
            "initial": { "type": "number" },
            "taper": { "type": "number" },
            "terminal": { "type": "number" },
        }
    }),
    resultFactory('epochInfo', {
        "required": ["epoch", "slotIndex", "slotsInEpoch", "absoluteSlot"],
        "properties": {
            "epoch": { "type": "number" },
            "slotIndex": { "type": "number" },
            "slotsInEpoch": { "type": "number" },
            "absoluteSlot": { "type": "number" },
            "blockHeight": { "type": "number" },
        }
    }),
    resultFactory('epochSchedule', {
        "required": ["slotsPerEpoch", "leaderScheduleSlotOffset", "warmup", "firstNormalEpoch", "firstNormalSlot"],
        "properties": {
            "slotsPerEpoch": { "type": "number" },
            "leaderScheduleSlotOffset": { "type": "number" },
            "warmup": { "type": "boolean" },
            "firstNormalEpoch": { "type": "number" },
            "firstNormalSlot": { "type": "number" },
        }
    }),
    contextFactory('tokenAccountBalance', {
        "required": [ "amount", "uiAmount", "decimals" ],
        "properties": {
            "amount": { "type": "string" },
            "uiAmount": { "type": "integer" },
            "decimals": { "type": "integer" },
        }
    }),
    resultFactory('leaderSchedule', {
        "additionalProperties": {
            "type": "array",
            "items": {
                "type": "number"
            }
        }
    }),
    resultFactory('confirmedSignaturesForAddress2', {
        "type": "array",
        "items": {
            "required": ["signature", "slot"],
            "properties": {
                "signature": { "type": "string" },
                "slot": { "type": "number" },
                "err": { "type": "object" },
                "memo": { "type": "string" }
            }
        }
    }),
    resultFactory('clusterNodes', {
        "type": "array",
        "items": {
            "required": ["pubkey"],
            "properties": {
                "pubkey": { "type": "string" },
                "gossip": { "type": "string" },
                "tpu": { "type": "string" },
                "rpc": { "type": "string" },
                "version": { "type": "string" },
            }
        }
    }),
    contextFactory('recentBlockHash', {
        "required": [ "blockhash", "feeCalculator" ],
        "properties": {
            "blockhash": { "type": "string" },
            "feeCalculator": { 
                "required": [ "lamportsPerSignature" ],
                "properties": {
                    "lamportsPerSignature": { "type": "number"}
                }
             },
        }
    }),
    contextFactory('feeCalculator', {
        "properties": {
            "feeCalculator": { 
                "required": [ "lamportsPerSignature" ],
                "properties": {
                    "lamportsPerSignature": { "type": "number"}
                }
             },
        }
    }),
    notificationFactory('accountNotification', {
        ...accountInfo
    }),
];
