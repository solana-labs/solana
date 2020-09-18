export const init = async () => {
    // @ts-ignore
    await import('./../Cargo.toml').then(instance => console.log('x'));
};

// imports.setup();