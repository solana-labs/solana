declare module 'superstruct' {
  declare type StructFunc = {
      (any): any,
      object(schema: any): any;
      union(schema: any): any;
      array(schema: any): any;
      literal(schema: any): any;
      tuple(schema: any): any;
      pick(schema: any): any;
      record(schema: any): any;
  };

  declare module.exports: {
    struct: StructFunc;
  }
}
