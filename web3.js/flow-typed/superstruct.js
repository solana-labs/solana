declare module 'superstruct' {
  declare type StructFunc = {
      (any): any,
      union(schema: any): any;
      list(schema: any): any;
      literal(schema: any): any;

  };

  declare module.exports: {
    struct: StructFunc;
  }
}
