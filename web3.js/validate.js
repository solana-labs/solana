'use strict';
var equal = require('ajv/lib/compile/equal');
var validate = (function() {
  var refVal = [];
  var refVal1 = {
    "required": ["result"],
    "properties": {
      "result": {
        "required": ["context", "value"],
        "context": {
          "required": ["slot"],
          "properties": {
            "slot": {
              "type": "integer"
            }
          }
        },
        "value": {
          "type": "object",
          "required": ["executable", "lamports", "owner", "data"],
          "properties": {
            "data": {
              "type": "object",
              "required": ["space", "program", "parsed"],
              "properties": {
                "parsed": {
                  "type": "object"
                },
                "program": {
                  "type": "string"
                },
                "space": {
                  "type": "integer"
                }
              }
            },
            "executable": {
              "type": "boolean"
            },
            "lamports": {
              "type": "integer"
            },
            "owner": {
              "type": "string"
            },
            "rentEpoch": {
              "type": "integer"
            }
          }
        }
      }
    }
  };
  refVal[1] = refVal1;
  var refVal2 = (function() {
    return function validate(data, dataPath, parentData, parentDataProperty, rootData) {
      'use strict';
      var vErrors = null;
      var errors = 0;
      if (rootData === undefined) rootData = data;
      var errs_1 = errors;
      var errs_2 = errors;
      if ((data && typeof data === "object" && !Array.isArray(data))) {
        if (true) {
          var errs__2 = errors;
          var valid3 = true;
          if (data.id === undefined) {
            valid3 = false;
            validate.errors = [{
              keyword: 'required',
              dataPath: (dataPath || '') + "",
              schemaPath: '#/definitions/common/required',
              params: {
                missingProperty: 'id'
              },
              message: 'should have required property \'id\''
            }];
            return false;
          } else {
            var errs_3 = errors;
            if (typeof data.id !== "string") {
              validate.errors = [{
                keyword: 'type',
                dataPath: (dataPath || '') + '.id',
                schemaPath: '#/definitions/common/properties/id/type',
                params: {
                  type: 'string'
                },
                message: 'should be string'
              }];
              return false;
            }
            var valid3 = errors === errs_3;
          }
          if (valid3) {
            if (data.jsonrpc === undefined) {
              valid3 = false;
              validate.errors = [{
                keyword: 'required',
                dataPath: (dataPath || '') + "",
                schemaPath: '#/definitions/common/required',
                params: {
                  missingProperty: 'jsonrpc'
                },
                message: 'should have required property \'jsonrpc\''
              }];
              return false;
            } else {
              var errs_3 = errors;
              var schema3 = refVal3.properties.jsonrpc.enum;
              var valid3;
              valid3 = false;
              for (var i3 = 0; i3 < schema3.length; i3++)
                if (equal(data.jsonrpc, schema3[i3])) {
                  valid3 = true;
                  break;
                } if (!valid3) {
                validate.errors = [{
                  keyword: 'enum',
                  dataPath: (dataPath || '') + '.jsonrpc',
                  schemaPath: '#/definitions/common/properties/jsonrpc/enum',
                  params: {
                    allowedValues: schema3
                  },
                  message: 'should be equal to one of the allowed values'
                }];
                return false;
              } else {}
              if (errors === errs_3) {}
              var valid3 = errors === errs_3;
            }
            if (valid3) {}
          }
          if (errs__2 == errors) {}
        }
      } else {
        validate.errors = [{
          keyword: 'type',
          dataPath: (dataPath || '') + "",
          schemaPath: '#/definitions/common/type',
          params: {
            type: 'object'
          },
          message: 'should be object'
        }];
        return false;
      }
      if (errors === errs_2) {
        var errs__2 = errors;
        var errs_3 = errors;
        if ((data && typeof data === "object" && !Array.isArray(data))) {
          var missing3;
          if (((data.result === undefined) && (missing3 = '.result')) || ((data.error === undefined) && (missing3 = '.error'))) {
            var err = {};
            if (vErrors === null) vErrors = [err];
            else vErrors.push(err);
            errors++;
          } else {}
        }
        if (errors === errs_3) {}
        var valid3 = errors === errs_3;
        if (valid3) {
          validate.errors = [{
            keyword: 'not',
            dataPath: (dataPath || '') + "",
            schemaPath: '#/definitions/common/not',
            params: {},
            message: 'should NOT be valid'
          }];
          return false;
        } else {
          errors = errs__2;
          if (vErrors !== null) {
            if (errs__2) vErrors.length = errs__2;
            else vErrors = null;
          }
        }
        if (errors === errs_2) {}
      }
      var valid2 = errors === errs_2;
      if (valid2) {}
      if (errors === errs_1) {}
      var valid1 = errors === errs_1;
      if (valid1) {
        var errs_1 = errors;
        if ((data && typeof data === "object" && !Array.isArray(data))) {
          if (true) {
            var errs__1 = errors;
            var valid2 = true;
            var data1 = data.error;
            if (data1 === undefined) {
              valid2 = false;
              validate.errors = [{
                keyword: 'required',
                dataPath: (dataPath || '') + "",
                schemaPath: '#/allOf/1/required',
                params: {
                  missingProperty: 'error'
                },
                message: 'should have required property \'error\''
              }];
              return false;
            } else {
              var errs_2 = errors;
              if ((data1 && typeof data1 === "object" && !Array.isArray(data1))) {
                if (true) {
                  var errs__2 = errors;
                  var valid3 = true;
                  var data2 = data1.code;
                  if (data2 === undefined) {
                    valid3 = false;
                    validate.errors = [{
                      keyword: 'required',
                      dataPath: (dataPath || '') + '.error',
                      schemaPath: '#/allOf/1/properties/error/required',
                      params: {
                        missingProperty: 'code'
                      },
                      message: 'should have required property \'code\''
                    }];
                    return false;
                  } else {
                    var errs_3 = errors;
                    if ((typeof data2 !== "number" || (data2 % 1) || data2 !== data2)) {
                      validate.errors = [{
                        keyword: 'type',
                        dataPath: (dataPath || '') + '.error.code',
                        schemaPath: '#/allOf/1/properties/error/properties/code/type',
                        params: {
                          type: 'integer'
                        },
                        message: 'should be integer'
                      }];
                      return false;
                    }
                    var valid3 = errors === errs_3;
                  }
                  if (valid3) {
                    if (data1.message === undefined) {
                      valid3 = false;
                      validate.errors = [{
                        keyword: 'required',
                        dataPath: (dataPath || '') + '.error',
                        schemaPath: '#/allOf/1/properties/error/required',
                        params: {
                          missingProperty: 'message'
                        },
                        message: 'should have required property \'message\''
                      }];
                      return false;
                    } else {
                      var errs_3 = errors;
                      if (typeof data1.message !== "string") {
                        validate.errors = [{
                          keyword: 'type',
                          dataPath: (dataPath || '') + '.error.message',
                          schemaPath: '#/allOf/1/properties/error/properties/message/type',
                          params: {
                            type: 'string'
                          },
                          message: 'should be string'
                        }];
                        return false;
                      }
                      var valid3 = errors === errs_3;
                    }
                    if (valid3) {
                      if (valid3) {}
                    }
                  }
                  if (errs__2 == errors) {}
                }
              } else {
                validate.errors = [{
                  keyword: 'type',
                  dataPath: (dataPath || '') + '.error',
                  schemaPath: '#/allOf/1/properties/error/type',
                  params: {
                    type: 'object'
                  },
                  message: 'should be object'
                }];
                return false;
              }
              if (errors === errs_2) {}
              var valid2 = errors === errs_2;
            }
            if (valid2) {}
            if (errs__1 == errors) {}
          }
        }
        if (errors === errs_1) {}
        var valid1 = errors === errs_1;
        if (valid1) {}
      }
      if (errors === 0) {}
      validate.errors = vErrors;
      return errors === 0;
    };
  })();
  refVal2.schema = {
    "allOf": [{
      "$ref": "#/definitions/common"
    }, {
      "required": ["error"],
      "properties": {
        "error": {
          "type": "object",
          "required": ["code", "message"],
          "properties": {
            "code": {
              "type": "integer",
              "note": ["unenforceable in some languages"]
            },
            "message": {
              "type": "string"
            },
            "data": {
              "description": "optional, can be anything"
            }
          }
        }
      }
    }]
  };
  refVal2.errors = null;
  refVal[2] = refVal2;
  var refVal3 = {
    "required": ["id", "jsonrpc"],
    "not": {
      "required": ["result", "error"]
    },
    "type": "object",
    "properties": {
      "id": {
        "type": ["string"]
      },
      "jsonrpc": {
        "enum": ["2.0"]
      }
    }
  };
  refVal[3] = refVal3;
  return function validate(data, dataPath, parentData, parentDataProperty, rootData) {
    'use strict'; /*# sourceURL=accountInfo */
    var vErrors = null;
    var errors = 0;
    if (rootData === undefined) rootData = data;
    var errs__0 = errors,
      prevValid0 = false,
      valid0 = false,
      passingSchemas0 = null;
    var errs_1 = errors;
    var errs_2 = errors;
    if ((data && typeof data === "object" && !Array.isArray(data))) {
      if (true) {
        var errs__2 = errors;
        var valid3 = true;
        var data1 = data.result;
        if (data1 === undefined) {
          valid3 = false;
          var err = {
            keyword: 'required',
            dataPath: (dataPath || '') + "",
            schemaPath: '#/definitions/success/required',
            params: {
              missingProperty: 'result'
            },
            message: 'should have required property \'result\''
          };
          if (vErrors === null) vErrors = [err];
          else vErrors.push(err);
          errors++;
        } else {
          var errs_3 = errors;
          if ((data1 && typeof data1 === "object" && !Array.isArray(data1))) {
            var missing3;
            if (((data1.context === undefined) && (missing3 = '.context')) || ((data1.value === undefined) && (missing3 = '.value'))) {
              var err = {
                keyword: 'required',
                dataPath: (dataPath || '') + '.result',
                schemaPath: '#/definitions/success/properties/result/required',
                params: {
                  missingProperty: '' + missing3 + ''
                },
                message: 'should have required property \'' + missing3 + '\''
              };
              if (vErrors === null) vErrors = [err];
              else vErrors.push(err);
              errors++;
            } else {}
          }
          if (errors === errs_3) {}
          var valid3 = errors === errs_3;
        }
        if (valid3) {}
        if (errs__2 == errors) {}
      }
    }
    if (errors === errs_2) {}
    var valid2 = errors === errs_2;
    if (valid2) {}
    if (errors === errs_1) {}
    var valid1 = errors === errs_1;
    if (valid1) {
      valid0 = prevValid0 = true;
      passingSchemas0 = 0;
    }
    var errs_1 = errors;
    if (!refVal2(data, (dataPath || ''), parentData, parentDataProperty, rootData)) {
      if (vErrors === null) vErrors = refVal2.errors;
      else vErrors = vErrors.concat(refVal2.errors);
      errors = vErrors.length;
    } else {}
    if (errors === errs_1) {}
    var valid1 = errors === errs_1;
    if (valid1 && prevValid0) {
      valid0 = false;
      passingSchemas0 = [passingSchemas0, 1];
    } else {
      if (valid1) {
        valid0 = prevValid0 = true;
        passingSchemas0 = 1;
      }
    }
    if (!valid0) {
      var err = {
        keyword: 'oneOf',
        dataPath: (dataPath || '') + "",
        schemaPath: '#/oneOf',
        params: {
          passingSchemas: passingSchemas0
        },
        message: 'should match exactly one schema in oneOf'
      };
      if (vErrors === null) vErrors = [err];
      else vErrors.push(err);
      errors++;
      validate.errors = vErrors;
      return false;
    } else {
      errors = errs__0;
      if (vErrors !== null) {
        if (errs__0) vErrors.length = errs__0;
        else vErrors = null;
      }
    }
    if (errors === 0) {}
    validate.errors = vErrors;
    return errors === 0;
  };
})();
validate.schema = {
  "$id": "accountInfo",
  "description": "A JSON RPC 2.0 response",
  "oneOf": [{
    "$ref": "#/definitions/success"
  }, {
    "$ref": "#/definitions/error"
  }],
  "definitions": {
    "common": {
      "required": ["id", "jsonrpc"],
      "not": {
        "required": ["result", "error"]
      },
      "type": "object",
      "properties": {
        "id": {
          "type": ["string"]
        },
        "jsonrpc": {
          "enum": ["2.0"]
        }
      }
    },
    "success": {
      "required": ["result"],
      "properties": {
        "result": {
          "required": ["context", "value"],
          "context": {
            "required": ["slot"],
            "properties": {
              "slot": {
                "type": "integer"
              }
            }
          },
          "value": {
            "type": "object",
            "required": ["executable", "lamports", "owner", "data"],
            "properties": {
              "data": {
                "type": "object",
                "required": ["space", "program", "parsed"],
                "properties": {
                  "parsed": {
                    "type": "object"
                  },
                  "program": {
                    "type": "string"
                  },
                  "space": {
                    "type": "integer"
                  }
                }
              },
              "executable": {
                "type": "boolean"
              },
              "lamports": {
                "type": "integer"
              },
              "owner": {
                "type": "string"
              },
              "rentEpoch": {
                "type": "integer"
              }
            }
          }
        }
      }
    },
    "error": {
      "allOf": [{
        "$ref": "#/definitions/common"
      }, {
        "required": ["error"],
        "properties": {
          "error": {
            "type": "object",
            "required": ["code", "message"],
            "properties": {
              "code": {
                "type": "integer",
                "note": ["unenforceable in some languages"]
              },
              "message": {
                "type": "string"
              },
              "data": {
                "description": "optional, can be anything"
              }
            }
          }
        }
      }]
    }
  }
};
validate.errors = null;
module.exports = validate;