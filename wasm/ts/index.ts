export const init = async () => {
    // @ts-ignore
    await import('./../Cargo.toml').then(instance => instance.default()).then(lib => console.log(lib));
};

// imports.setup();