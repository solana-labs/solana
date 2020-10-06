const jsonRPCResultBuilder = (name, resultSchema) => ({
    "$id": name,
    "description": "A JSON RPC 2.0 response",
    "oneOf": [
        { "$ref": "#/definitions/success" },
        { "$ref": "#/definitions/error" }
    ],
    "definitions": {
        "common": {
            "required": [ "id", "jsonrpc" ],
            "not": { "required": [ "result", "error" ] },
            "type": "object",
            "properties": {
                "id": { "type": [ "string"  ] },
                "jsonrpc": { "enum": [ "2.0" ] }
            }
        },
        "success": {
            "required": [ "result" ], "properties": { "result": resultSchema }
        },
        "error": {
            "allOf" : [
                { "$ref": "#/definitions/common" },
                {
                    "required": [ "error" ],
                    "properties": {
                        "error": {
                            "type": "object",
                            "required": [ "code", "message" ],
                            "properties": {
                                "code": {
                                    "type": "integer",
                                    "note": [ "unenforceable in some languages" ]
                                },
                                "message": { "type": "string" },
                                "data": {
                                    "description": "optional, can be anything"
                                }
                            }
                        }
                    }
                }
            ]
        }
    }
});

module.exports = [
    jsonRPCResultBuilder('stakeActivation', { 
        "required": [ "state", "active", "inactive" ],
        "properties": {
            "state": { "enum": [ 'active','inactive','activating','deactivating', ] },
            "active": { "type": "integer" },
            "inactive": { "type": "integer" },
        }
    }),
    jsonRPCResultBuilder('accountInfo', { 
        "required": [ "context", "value" ],
        "context": { "required": [ "slot" ], "properties": { "slot": { "type": "integer" } } },
        "value": {
            "type": "object",
            "required": [ "executable", "lamports", "owner", "data" ],
            "properties": {
                "data": {
                    "type": "object",
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
        }
    }),
    jsonRPCResultBuilder('leaderSchedule', {
        "additionalProperties": {
            "type": "array",
            "items": {
            "type": "number"
            }
        }
    }),
];
