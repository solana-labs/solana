import { Program } from "@project-serum/anchor";

export function getProgramName(program: Program | null): string | undefined {
    return program ? capitalizeFirstLetter(program.idl.name) : undefined
}

export function capitalizeFirstLetter(input: string) {
    return input.charAt(0).toUpperCase() + input.slice(1);
}