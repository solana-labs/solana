declare module 'superstruct' {
  declare type StructFunc = {
      (any): any,
      union(schema: any): any;
      array(schema: any): any;
      literal(schema: any): any;
      tuple(schema: any): any;
  };

  declare module.exports: {
    struct: StructFunc;
  }
}
